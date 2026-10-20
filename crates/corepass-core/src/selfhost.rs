//! Server management runs independently of the local VPN engine.
//! It never stops a tunnel or changes Windows networking, including on failure.
use crate::logging::Journal;
use corepass_selfhost::Manager;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroize;

const RESULT_LIFETIME: Duration = Duration::from_secs(300);

struct Job {
    value: Value,
    completed: Option<Instant>,
}
impl Drop for Job {
    fn drop(&mut self) {
        erase_strings(&mut self.value);
    }
}
pub struct Service {
    manager: Arc<Manager>,
    jobs: Mutex<VecDeque<Job>>,
    busy: AtomicBool,
    log: Arc<Journal>,
}

impl Service {
    pub fn new(data: PathBuf, log: Arc<Journal>) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            manager: Arc::new(Manager::new(data)?),
            jobs: Mutex::new(VecDeque::new()),
            busy: AtomicBool::new(false),
            log,
        }))
    }

    pub fn list(&self) -> Result<Value, String> {
        let mut value = self.manager.request("GET", "/v1/servers", &json!({}))?;
        value["busy"] = json!(self.busy.load(Ordering::SeqCst));
        value["feature"] = json!("preview");
        Ok(value)
    }

    pub fn purge_expired(&self) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        for job in jobs.iter_mut() {
            if job.completed.is_some_and(|t| t.elapsed() > RESULT_LIFETIME)
                && job.value["state"] != "expired"
            {
                let id = job.value["id"].clone();
                erase_strings(&mut job.value);
                job.value = json!({"id":id,"state":"expired","error":"Результат очищен. Повторите действие, если нужен новый QR-код."});
            }
        }
    }

    pub fn operation(&self, id: &str) -> Result<Value, String> {
        self.purge_expired();
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs
            .iter()
            .find(|j| j.value["id"] == id)
            .ok_or("Операция сервера не найдена")?;
        Ok(job.value.clone())
    }

    pub fn submit(self: &Arc<Self>, path: &str, mut body: Value) -> Result<Value, String> {
        if let Err(error) = validate_request(path, &body) {
            erase_password(&mut body);
            return Err(error);
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            erase_password(&mut body);
            return Err("Уже выполняется операция сервера. Дождитесь её завершения.".into());
        }
        let id = format!("srv_{}", uuid::Uuid::new_v4().simple());
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            while jobs.len() >= 12 {
                if let Some(mut job) = jobs.pop_front() {
                    erase_strings(&mut job.value);
                }
            }
            jobs.push_back(Job {
                value: json!({"id":id,"action":path,"state":"running"}),
                completed: None,
            });
        }
        let service = self.clone();
        let action = path.to_string();
        self.log.event("info", "SERVER_OP_STARTED", &action);
        let job_id = id.clone();
        std::thread::spawn(move || {
            let mut body = body;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                service.manager.request("POST", &action, &body)
            }))
            .unwrap_or_else(|_| Err("Внутренняя ошибка управления сервером".into()));
            erase_password(&mut body);
            {
                let mut jobs = service.jobs.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = jobs.iter_mut().find(|j| j.value["id"] == job_id) {
                    match result {
                        Ok(value) => {
                            job.value["state"] = json!("succeeded");
                            job.value["result"] = value;
                            service.log.event("info", "SERVER_OP_DONE", &action);
                        }
                        Err(message) => {
                            job.value["state"] = json!("failed");
                            job.value["error"] = json!(message);
                            service.log.event("error", "CP-SERVER-OP", &format!("{action}: операция сервера завершилась ошибкой; детали доступны в мастере"));
                        }
                    }
                    job.completed = Some(Instant::now());
                }
            }
            service.busy.store(false, Ordering::SeqCst);
        });
        Ok(json!({"operation_id":id,"state":"accepted"}))
    }
}

fn erase_password(body: &mut Value) {
    if let Some(Value::String(password)) = body.get_mut("password") {
        password.zeroize();
    }
}

fn erase_strings(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(erase_strings),
        Value::Object(items) => items.values_mut().for_each(erase_strings),
        _ => (),
    }
}

fn validate_request(path: &str, body: &Value) -> Result<(), String> {
    corepass_selfhost::validate_request("POST", path, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn management_rejects_arbitrary_commands_before_any_worker() {
        assert!(validate_request("/v1/servers/exec", &json!({"command":"anything"})).is_err());
        assert!(validate_request(
            "/v1/servers/inspect",
            &json!({"host":"example.com", "command":"anything"})
        )
        .is_err());
        assert!(validate_request(
            "/v1/servers/host-key",
            &json!({"host":"example.com", "ssh_port":0})
        )
        .is_err());
        assert!(validate_request(
            "/v1/servers/host-key",
            &json!({"host":"example.com", "ssh_port":22})
        )
        .is_ok());
    }
    #[test]
    fn exported_credentials_expire_without_touching_local_network() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(
            dir.path().to_path_buf(),
            Arc::new(Journal::new(&dir.path().join("logs"))),
        )
        .unwrap();
        service.jobs.lock().unwrap().push_back(Job {
            value: json!({"id":"srv_old","state":"succeeded","result":{"uri":"private-value"}}),
            completed: Some(Instant::now() - Duration::from_secs(301)),
        });
        let expired = service.operation("srv_old").unwrap();
        assert_eq!(expired["state"], "expired");
        assert!(!expired.to_string().contains("private-value"));
        assert_eq!(service.list().unwrap()["busy"], false);
    }
    #[test]
    fn password_value_is_erased_after_worker() {
        let mut body = json!({"password":"transient-example", "host":"example.com"});
        erase_password(&mut body);
        assert!(!body.to_string().contains("transient-example"));
        assert_eq!(body["host"], "example.com");
    }
}
