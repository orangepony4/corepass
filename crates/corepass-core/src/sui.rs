//! S-UI management is independent from the local VPN engine and its command lock.
//! Only explicit client creation can write to S-UI; opening the page is local.
use crate::logging::Journal;
use crate::sui_credentials;
use corepass_sui::Manager;
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
        let mut value = self.manager.request("GET", "/v1/sui/panels", &json!({}))?;
        if let Some(panels) = value["panels"].as_array_mut() {
            for panel in panels {
                let saved = panel["id"].as_str().is_some_and(sui_credentials::has);
                panel["credential_saved"] = json!(saved);
            }
        }
        value["busy"] = json!(self.busy.load(Ordering::SeqCst));
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
            .ok_or("Операция S-UI не найдена")?;
        Ok(job.value.clone())
    }

    pub fn submit(self: &Arc<Self>, path: &str, mut body: Value) -> Result<Value, String> {
        if let Err(error) = self.prepare_request(path, &mut body) {
            erase_strings(&mut body);
            return Err(error);
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            erase_strings(&mut body);
            return Err("Уже выполняется операция S-UI. Дождитесь её завершения.".into());
        }
        let id = format!("sui_{}", uuid::Uuid::new_v4().simple());
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            while jobs.len() >= 12 {
                jobs.pop_front();
            }
            jobs.push_back(Job {
                value: json!({"id":id,"action":path,"state":"running"}),
                completed: None,
            });
        }
        let service = self.clone();
        // The path is validated against a fixed route list. Never journal bodies,
        // panel paths, remote messages, tokens or exported links.
        let action = path.to_string();
        self.log.event("info", "SUI_OP_STARTED", &action);
        let job_id = id.clone();
        std::thread::spawn(move || {
            let mut body = body;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match action.as_str() {
                    "/v1/sui/credentials/save" => {
                        sui_credentials::save(
                            body["panel_id"].as_str().unwrap(),
                            body["token"].as_str().unwrap(),
                        )?;
                        Ok(json!({"credential_saved":true}))
                    }
                    "/v1/sui/credentials/forget" => {
                        sui_credentials::forget(body["panel_id"].as_str().unwrap())?;
                        Ok(json!({"credential_saved":false}))
                    }
                    "/v1/sui/panels/save" => {
                        // A token belongs to the exact panel endpoint. Even an
                        // explicit URL re-entry requires a fresh token. Forget
                        // before updating, and refuse the update on vault error.
                        // Name/read-only/public VPN changes preserve the token.
                        if let Some(id) = credential_reset_id(&body) {
                            if sui_credentials::has(id) {
                                sui_credentials::forget(id)?;
                            }
                        }
                        service.manager.request("POST", &action, &body)
                    }
                    _ => service.manager.request("POST", &action, &body),
                }
            }))
            .unwrap_or_else(|_| Err("Внутренняя ошибка управления S-UI".into()));
            erase_strings(&mut body);
            {
                let mut jobs = service.jobs.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = jobs.iter_mut().find(|j| j.value["id"] == job_id) {
                    match result {
                        Ok(value) => {
                            job.value["state"] = json!("succeeded");
                            job.value["result"] = value;
                            service.log.event("info", "SUI_OP_DONE", &action);
                        }
                        Err(message) => {
                            job.value["state"] = json!("failed");
                            job.value["error"] = json!(message);
                            service.log.event("error", "CP-SUI-OP", "Операция S-UI завершилась ошибкой; детали доступны в разделе панели");
                        }
                    }
                    job.completed = Some(Instant::now());
                }
            }
            service.busy.store(false, Ordering::SeqCst);
        });
        Ok(json!({"operation_id":id,"state":"accepted"}))
    }

    fn prepare_request(&self, path: &str, body: &mut Value) -> Result<(), String> {
        if matches!(
            path,
            "/v1/sui/credentials/save" | "/v1/sui/credentials/forget"
        ) {
            let object = body.as_object().ok_or("Нужен JSON-объект")?;
            let allowed: &[&str] = if path.ends_with("/save") {
                &["panel_id", "token"]
            } else {
                &["panel_id"]
            };
            if object.keys().any(|key| !allowed.contains(&key.as_str())) {
                return Err("Неизвестные параметры хранилища S-UI".into());
            }
            let id = body["panel_id"].as_str().ok_or("Нужен panel_id")?;
            // Only existing, validated manager IDs can be used as credential keys.
            let panels = self.manager.request("GET", "/v1/sui/panels", &json!({}))?;
            if !panels["panels"]
                .as_array()
                .is_some_and(|panels| panels.iter().any(|p| p["id"] == id))
            {
                return Err("Панель не найдена".into());
            }
            if path.ends_with("/save") {
                let token = body["token"].as_str().ok_or("Нужен API-токен S-UI")?;
                if token.is_empty()
                    || token.len() > 2048
                    || !token.bytes().all(|b| (0x21..=0x7e).contains(&b))
                {
                    return Err("Некорректный API-токен S-UI".into());
                }
            }
            return Ok(());
        }
        if matches!(
            path,
            "/v1/sui/inspect"
                | "/v1/sui/clients/list"
                | "/v1/sui/clients/export"
                | "/v1/sui/clients/create"
        ) && body.get("token").is_none()
        {
            let id = body["panel_id"].as_str().ok_or("Нужен panel_id")?;
            let token = sui_credentials::load(id)?;
            body["token"] = json!(token.as_str());
        }
        corepass_sui::validate_request("POST", path, body)
    }
}

fn credential_reset_id(body: &Value) -> Option<&str> {
    body.get("base_url")?;
    body.get("panel_id")?.as_str()
}

fn erase_strings(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(erase_strings),
        Value::Object(items) => items.values_mut().for_each(erase_strings),
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_result_expires_without_leaking_into_journal() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Journal::new(&dir.path().join("logs")));
        let service = Service::new(dir.path().join("sui"), log.clone()).unwrap();
        service.jobs.lock().unwrap().push_back(Job {
            value: json!({"id":"sui_old","state":"succeeded","result":{"client":{"uri":"private-uri"}}}),
            completed: Some(Instant::now() - Duration::from_secs(301)),
        });
        let expired = service.operation("sui_old").unwrap();
        assert_eq!(expired["state"], "expired");
        assert!(!expired.to_string().contains("private-uri"));
        assert!(log.read().is_empty());
        assert_eq!(service.list().unwrap()["busy"], false);
    }

    #[test]
    fn rejected_and_busy_requests_never_log_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(Journal::new(&dir.path().join("logs")));
        let service = Service::new(dir.path().join("sui"), log.clone()).unwrap();
        assert!(service
            .submit("/v1/sui/restart", json!({"token":"secret-example"}))
            .is_err());
        assert!(service.jobs.lock().unwrap().is_empty());
        assert!(!service.busy.load(Ordering::SeqCst));
        assert!(log.read().is_empty());
        let mut body = json!({"token":"secret-example","nested":[{"password":"example-pass"}],"base_url":"https://example.com/private-panel/"});
        erase_strings(&mut body);
        assert!(!body.to_string().contains("example"));
    }

    #[test]
    fn changing_panel_endpoint_requires_a_fresh_credential() {
        assert_eq!(
            credential_reset_id(
                &json!({"panel_id":"owned","base_url":"https://new.example.com/private/"})
            ),
            Some("owned")
        );
        assert_eq!(
            credential_reset_id(
                &json!({"panel_id":"owned","name":"New name","read_only":false,"server_domain":"vpn.example.com"})
            ),
            None
        );
        assert_eq!(
            credential_reset_id(&json!({"base_url":"https://new.example.com/"})),
            None
        );
    }
}
