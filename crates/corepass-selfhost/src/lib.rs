//! Bounded SSH control plane. No constructor or inventory operation opens a socket.
//! Passwords are accepted for one operation and are never serialized or logged here.

use russh::{
    client,
    keys::{HashAlg, PublicKeyOrCertificate},
    ChannelMsg, Disconnect,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const PLAN_TTL: Duration = Duration::from_secs(600);
const OUTPUT_LIMIT: usize = 262_144;
const MAX_SERVERS: usize = 16;
const CONTROLLER: &str = include_str!("../../../server/controller.py");
const PYTHON_BOOTSTRAP: &str = r#"import sys; sys.stdout.reconfigure(encoding="utf-8"); r=sys.stdin.buffer; n=int(r.readline(16)); source=r.read(n); assert len(source)==n; exec(compile(source,"<corepass-controller>","exec"),globals())"#;

fn controller_frame(source: &str, request: &Value) -> Result<Vec<u8>, String> {
    let mut input = format!("{}\n", source.len()).into_bytes();
    input.extend_from_slice(source.as_bytes());
    serde_json::to_writer(&mut input, request).map_err(|_| "Ошибка запроса сервера")?;
    input.push(b'\n');
    Ok(input)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    host: String,
    ssh_port: u16,
    username: String,
    domain: String,
    fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Server {
    id: String,
    name: String,
    #[serde(flatten)]
    target: Target,
    created_at: String,
}

struct Connection {
    target: Target,
    password: Zeroizing<String>,
}

struct Plan {
    id: String,
    target: Target,
    expires: Instant,
    ready: bool,
}

#[derive(Clone)]
struct HostKey {
    fingerprint: String,
    algorithm: String,
}

trait Transport: Send + Sync {
    fn discover(&self, host: &str, port: u16) -> Result<HostKey, String>;
    fn execute(&self, connection: &Connection, request: &Value) -> Result<Value, String>;
}

pub struct Manager {
    data: PathBuf,
    servers: Mutex<Vec<Server>>,
    plans: Mutex<Vec<Plan>>,
    mutation: Mutex<()>,
    transport: Arc<dyn Transport>,
}

fn field<'a>(body: &'a Value, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Нужно поле {key}"))
}

fn host_valid(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.trim() != host {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast();
    }
    hostname_valid(host) && host != "localhost" && !host.ends_with(".localhost")
}

fn hostname_valid(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn domain_valid(domain: &str) -> bool {
    hostname_valid(domain)
        && domain.contains('.')
        && domain.parse::<IpAddr>().is_err()
        && domain != "localhost"
        && !domain.ends_with(".localhost")
}

fn fingerprint_valid(fingerprint: &str) -> bool {
    let Some(hash) = fingerprint.strip_prefix("SHA256:") else {
        return false;
    };
    hash.len() == 43
        && hash
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/".contains(&b))
}

fn port(body: &Value) -> Result<u16, String> {
    match body.get("ssh_port") {
        None => Ok(22),
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|p| *p != 0)
            .ok_or_else(|| "Порт SSH должен быть от 1 до 65535".into()),
    }
}

fn parse_target(body: &Value) -> Result<Target, String> {
    let target = Target {
        host: field(body, "host")?.to_ascii_lowercase(),
        ssh_port: port(body)?,
        username: field(body, "username")?.into(),
        domain: field(body, "domain")?.to_ascii_lowercase(),
        fingerprint: field(body, "fingerprint")?.into(),
    };
    validate_target(&target)?;
    Ok(target)
}

fn validate_target(target: &Target) -> Result<(), String> {
    if !host_valid(&target.host) {
        return Err("Укажите адрес VPS без схемы, пути и команд".into());
    }
    if target.ssh_port == 0 {
        return Err("Некорректный порт SSH".into());
    }
    if target.username != "root" {
        return Err("Первая версия установки поддерживает вход SSH от root".into());
    }
    if !domain_valid(&target.domain) {
        return Err("Нужен полный домен для действительного HTTPS-сертификата".into());
    }
    if !fingerprint_valid(&target.fingerprint) {
        return Err("Сначала получите и подтвердите SHA256-отпечаток SSH".into());
    }
    Ok(())
}

fn validate_password(body: &Value) -> Result<(), String> {
    let value = field(body, "password")?;
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err("Укажите пароль SSH длиной до 4096 байт".into());
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

/// Validate the fixed schema before starting an asynchronous operation or opening a socket.
pub fn validate_request(method: &str, path: &str, body: &Value) -> Result<(), String> {
    let obj = body.as_object().ok_or("Нужен JSON-объект")?;
    let fields: &[&str] = match (method, path) {
        ("GET", "/v1/servers") => &[],
        ("POST", "/v1/servers/host-key") => &["host", "ssh_port"],
        ("POST", "/v1/servers/inspect") => &[
            "host",
            "ssh_port",
            "username",
            "password",
            "domain",
            "fingerprint",
        ],
        ("POST", "/v1/servers/install") => &[
            "host",
            "ssh_port",
            "username",
            "password",
            "domain",
            "fingerprint",
            "plan_id",
        ],
        ("POST", "/v1/servers/clients/list") => &["server_id", "password"],
        ("POST", "/v1/servers/clients/create") => &["server_id", "password", "client_name"],
        ("POST", "/v1/servers/clients/revoke" | "/v1/servers/clients/export") => {
            &["server_id", "password", "client_id"]
        }
        _ => return Err("Маршрут управления сервером не найден".into()),
    };
    if obj.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err("Неизвестные параметры команды сервера".into());
    }
    match path {
        "/v1/servers" => {}
        "/v1/servers/host-key" => {
            if !host_valid(field(body, "host")?) {
                return Err("Укажите адрес VPS без схемы и пути".into());
            }
            port(body)?;
        }
        "/v1/servers/inspect" | "/v1/servers/install" => {
            parse_target(body)?;
            validate_password(body)?;
            if path.ends_with("/install") && !valid_id(field(body, "plan_id")?) {
                return Err("Нужен действующий план установки".into());
            }
        }
        _ => {
            if !valid_id(field(body, "server_id")?) {
                return Err("Некорректный идентификатор сервера".into());
            }
            validate_password(body)?;
            if path.ends_with("/create") {
                let name = field(body, "client_name")?;
                if name.trim().is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                    return Err("Имя клиента: от 1 до 80 символов без управляющих знаков".into());
                }
            }
            if (path.ends_with("/revoke") || path.ends_with("/export"))
                && !valid_id(field(body, "client_id")?)
            {
                return Err("Некорректный идентификатор клиента".into());
            }
        }
    }
    Ok(())
}

impl Manager {
    pub fn new(data: PathBuf) -> Result<Self, String> {
        Self::with_transport(data, Arc::new(SshTransport))
    }

    fn with_transport(data: PathBuf, transport: Arc<dyn Transport>) -> Result<Self, String> {
        fs::create_dir_all(&data).map_err(|_| "Не создать каталог серверов")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&data, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Не защитить каталог серверов")?;
        }
        let store = data.join("servers.json");
        let servers: Vec<Server> = if store.exists() {
            let bytes = fs::read(&store).map_err(|_| "Не читается список серверов")?;
            if bytes.len() > 131_072 {
                return Err("Список серверов повреждён".into());
            }
            serde_json::from_slice(&bytes).map_err(|_| "Повреждён список серверов")?
        } else {
            vec![]
        };
        if servers.len() > MAX_SERVERS {
            return Err("Слишком много серверов".into());
        }
        for server in &servers {
            validate_target(&server.target)?;
            if !valid_id(&server.id) {
                return Err("Повреждён идентификатор сервера".into());
            }
        }
        Ok(Self {
            data,
            servers: Mutex::new(servers),
            plans: Mutex::new(vec![]),
            mutation: Mutex::new(()),
            transport,
        })
    }

    pub fn request(&self, method: &str, path: &str, body: &Value) -> Result<Value, String> {
        validate_request(method, path, body)?;
        if method == "GET" {
            return Ok(
                json!({"servers":*self.servers.lock().map_err(|_| "Ошибка списка серверов")?,"supported":true}),
            );
        }
        let _serial = self
            .mutation
            .try_lock()
            .map_err(|_| "Другая операция с сервером ещё выполняется")?;
        match path {
            "/v1/servers/host-key" => {
                let host = field(body, "host")?.to_ascii_lowercase();
                let ssh_port = port(body)?;
                let key = self.transport.discover(&host, ssh_port)?;
                Ok(
                    json!({"host":host,"ssh_port":ssh_port,"fingerprint":key.fingerprint,"algorithm":key.algorithm}),
                )
            }
            "/v1/servers/inspect" => {
                let connection = Connection {
                    target: parse_target(body)?,
                    password: Zeroizing::new(field(body, "password")?.into()),
                };
                let result = self.run(&connection, "inspect", None)?;
                let mut result = public_inspection(&result)?;
                let plan = Plan {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    target: connection.target,
                    expires: Instant::now() + PLAN_TTL,
                    ready: result["ready"] == true
                        && result["blockers"].as_array().is_some_and(Vec::is_empty),
                };
                result["plan_id"] = json!(plan.id);
                result["expires_in_seconds"] = json!(PLAN_TTL.as_secs());
                let mut plans = self.plans.lock().map_err(|_| "Ошибка планов установки")?;
                plans.retain(|p| p.expires > Instant::now());
                if plans.len() >= 8 {
                    plans.remove(0);
                }
                plans.push(plan);
                Ok(result)
            }
            "/v1/servers/install" => {
                let connection = Connection {
                    target: parse_target(body)?,
                    password: Zeroizing::new(field(body, "password")?.into()),
                };
                self.consume_plan(field(body, "plan_id")?, &connection.target)?;
                // Refuse local inventory conflicts before sending any mutating remote command.
                {
                    let servers = self.servers.lock().map_err(|_| "Ошибка списка серверов")?;
                    let existing = servers.iter().find(|s| {
                        s.target.host == connection.target.host
                            && s.target.ssh_port == connection.target.ssh_port
                            && s.target.domain == connection.target.domain
                    });
                    if existing
                        .is_some_and(|s| s.target.fingerprint != connection.target.fingerprint)
                    {
                        return Err("Отпечаток сохранённого сервера изменился. Автоматическое доверие запрещено".into());
                    }
                    if existing.is_none() && servers.len() >= MAX_SERVERS {
                        return Err("Достигнут предел 16 серверов".into());
                    }
                }
                let fresh = public_inspection(&self.run(&connection, "inspect", None)?)?;
                if fresh["ready"] != true
                    || !fresh["blockers"].as_array().is_some_and(Vec::is_empty)
                {
                    return Err(
                        "Состояние VPS изменилось. Повторите проверку перед установкой".into(),
                    );
                }
                let raw = self.run(&connection, "install", None)?;
                if raw["installed"] != true {
                    return Err("Сервер не подтвердил установку".into());
                }
                let mut servers = self.servers.lock().map_err(|_| "Ошибка списка серверов")?;
                let mut updated = servers.clone();
                let server = if let Some(existing) = updated.iter().find(|s| {
                    s.target.host == connection.target.host
                        && s.target.ssh_port == connection.target.ssh_port
                        && s.target.domain == connection.target.domain
                }) {
                    if existing.target.fingerprint != connection.target.fingerprint {
                        return Err("Отпечаток сохранённого сервера изменился. Автоматическое доверие запрещено".into());
                    }
                    existing.clone()
                } else {
                    if updated.len() >= MAX_SERVERS {
                        return Err("Достигнут предел 16 серверов".into());
                    }
                    Server {
                        id: uuid::Uuid::new_v4().simple().to_string(),
                        name: connection.target.domain.clone(),
                        target: connection.target,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    }
                };
                if !updated.iter().any(|s| s.id == server.id) {
                    updated.push(server.clone());
                }
                atomic_store(&self.data.join("servers.json"), &updated)?;
                *servers = updated;
                Ok(
                    json!({"server":server,"installed":true,"domain":server.target.domain,"service":bounded_text(raw.get("service"),40),"clients":public_clients(&raw)?}),
                )
            }
            _ => {
                let server_id = field(body, "server_id")?;
                let server = self
                    .servers
                    .lock()
                    .map_err(|_| "Ошибка списка серверов")?
                    .iter()
                    .find(|s| s.id == server_id)
                    .cloned()
                    .ok_or("Сервер не найден")?;
                let connection = Connection {
                    target: server.target,
                    password: Zeroizing::new(field(body, "password")?.into()),
                };
                let action = match path.rsplit('/').next().unwrap_or("") {
                    "list" => "clients.list",
                    "create" => "clients.create",
                    "revoke" => "clients.revoke",
                    "export" => "clients.export",
                    _ => return Err("Неизвестное действие".into()),
                };
                let result = self.run(&connection, action, Some(body))?;
                if action == "clients.create" || action == "clients.export" {
                    let public = public_client(&result["client"])?;
                    let uri = result["client"]["uri"]
                        .as_str()
                        .ok_or("Сервер не вернул ссылку клиента")?;
                    let parsed = url::Url::parse(uri).map_err(|_| "Некорректная ссылка клиента")?;
                    if uri.len() > 8192
                        || parsed.scheme() != "naive+https"
                        || parsed.host_str() != Some(&connection.target.domain)
                        || parsed.port().unwrap_or(443) != 443
                        || parsed.username().is_empty()
                        || parsed.password().is_none()
                    {
                        return Err("Ссылка клиента не соответствует этому HTTPS-серверу".into());
                    }
                    let mut client = public;
                    client["uri"] = json!(uri);
                    Ok(json!({"client":client}))
                } else {
                    Ok(
                        json!({"clients":public_clients(&result)?,"service":bounded_text(result.get("service"),40)}),
                    )
                }
            }
        }
    }

    fn consume_plan(&self, id: &str, target: &Target) -> Result<(), String> {
        let mut plans = self.plans.lock().map_err(|_| "Ошибка планов установки")?;
        plans.retain(|p| p.expires > Instant::now());
        let idx = plans
            .iter()
            .position(|p| p.id == id)
            .ok_or("План установки истёк или уже использован. Проверьте сервер заново")?;
        if plans[idx].target != *target || !plans[idx].ready {
            return Err("План не соответствует параметрам сервера или содержит препятствия".into());
        }
        plans.remove(idx);
        Ok(())
    }

    fn run(
        &self,
        connection: &Connection,
        action: &str,
        fields: Option<&Value>,
    ) -> Result<Value, String> {
        let mut request = json!({"action":action,"host":connection.target.host,"domain":connection.target.domain});
        for key in ["client_name", "client_id"] {
            if let Some(value) = fields.and_then(|v| v.get(key)) {
                request[key] = value.clone();
            }
        }
        let result = self.transport.execute(connection, &request)?;
        if result["ok"] != true {
            return Err(controller_error(&result, connection.password.as_str()));
        }
        Ok(result)
    }
}

fn controller_error(result: &Value, password: &str) -> String {
    // Treat even parsed controller errors as untrusted. The namespace is fixed,
    // and redaction covers both the code and the message before they leave here.
    let code = result["error"]["code"]
        .as_str()
        .filter(|code| {
            code.starts_with("CP-SERVER-")
                && code.len() > "CP-SERVER-".len()
                && code.len() <= 60
                && code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        })
        .unwrap_or("CP-SERVER-REMOTE");
    let message = bounded_text(result["error"].get("message"), 300);
    let combined = format!(
        "{code}: {}",
        if message.is_empty() {
            "Команда сервера отклонена"
        } else {
            &message
        }
    );
    if password.is_empty() {
        combined
    } else {
        combined.replace(password, "[скрыто]")
    }
}

fn bounded_text(value: Option<&Value>, maximum: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(maximum)
        .collect()
}

fn public_inspection(raw: &Value) -> Result<Value, String> {
    let checks = raw["checks"]
        .as_array()
        .ok_or("Неполный результат проверки VPS")?;
    let blockers = raw["blockers"]
        .as_array()
        .ok_or("Нет списка препятствий установки")?;
    if checks.len() > 40 || blockers.len() > 40 {
        return Err("Слишком большой результат проверки VPS".into());
    }
    Ok(json!({
        "ready":raw["ready"].as_bool().unwrap_or(false), "installed":raw["installed"].as_bool().unwrap_or(false),
        "checks":checks.iter().map(|c| json!({"id":bounded_text(c.get("id"),50),"label":bounded_text(c.get("label"),100),"ok":c["ok"].as_bool().unwrap_or(false),"message":bounded_text(c.get("message"),300)})).collect::<Vec<_>>(),
        "blockers":blockers.iter().map(|b| bounded_text(Some(b),300)).collect::<Vec<_>>(),
        "domain":bounded_text(raw.get("domain"),253), "host":bounded_text(raw.get("host"),253),
        "architecture":bounded_text(raw.get("architecture"),40), "os":bounded_text(raw.get("os"),100)
    }))
}

fn public_client(raw: &Value) -> Result<Value, String> {
    let id = raw["id"]
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or("Некорректный клиент в ответе VPS")?;
    Ok(
        json!({"id":id,"name":bounded_text(raw.get("name"),80),"created_at":bounded_text(raw.get("created_at"),60),"revoked":raw["revoked"].as_bool().unwrap_or(false)}),
    )
}

fn public_clients(raw: &Value) -> Result<Vec<Value>, String> {
    let clients = raw["clients"]
        .as_array()
        .ok_or("Сервер не вернул список клиентов")?;
    if clients.len() > 256 {
        return Err("Слишком много клиентов в ответе VPS".into());
    }
    clients.iter().map(public_client).collect()
}

fn atomic_store(path: &Path, value: &[Server]) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "Ошибка сохранения списка серверов")?;
    fs::write(&temporary, bytes).map_err(|_| "Не записать список серверов")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(|_| "Не защитить список серверов")?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            unsafe extern "system" {
                fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
            }
            if MoveFileExW(source.as_ptr(), destination.as_ptr(), 9) == 0 {
                return Err("Не заменить список серверов атомарно".into());
            }
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temporary, path).map_err(|_| "Не заменить список серверов атомарно")?;
    Ok(())
}

struct KeyHandler {
    expected: Option<String>,
    observed: Arc<Mutex<Option<HostKey>>>,
}

impl client::Handler for KeyHandler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public = key.public_key();
        let observed = HostKey {
            fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
            algorithm: public.algorithm().to_string(),
        };
        let accepted = self
            .expected
            .as_ref()
            .is_some_and(|pin| *pin == observed.fingerprint);
        *self.observed.lock().unwrap_or_else(|e| e.into_inner()) = Some(observed);
        // Discovery always refuses authentication: it records the signed key exchange only.
        Ok(accepted)
    }
}

struct SshTransport;
impl SshTransport {
    fn runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Не создать SSH-поток".into())
    }
    fn config() -> Arc<client::Config> {
        Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            maximum_packet_size: 32_768,
            window_size: 262_144,
            ..Default::default()
        })
    }
}
impl Transport for SshTransport {
    fn discover(&self, host: &str, port: u16) -> Result<HostKey, String> {
        let observed = Arc::new(Mutex::new(None));
        let handler = KeyHandler {
            expected: None,
            observed: observed.clone(),
        };
        let rt = Self::runtime()?;
        let outcome = rt.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(15),
                client::connect(Self::config(), (host, port), handler),
            )
            .await
        });
        if let Ok(Ok(session)) = outcome {
            drop(session);
        }
        let key = observed
            .lock()
            .map_err(|_| "Ошибка чтения отпечатка")?
            .clone();
        key.ok_or_else(|| {
            "CP-SSH-CONNECT: Не удалось получить ключ SSH за 15 секунд. Проверьте IP и порт VPS"
                .into()
        })
    }
    fn execute(&self, connection: &Connection, request: &Value) -> Result<Value, String> {
        let rt = Self::runtime()?;
        rt.block_on(async {
            let observed = Arc::new(Mutex::new(None));
            let handler = KeyHandler { expected: Some(connection.target.fingerprint.clone()), observed: observed.clone() };
            let mut session = match tokio::time::timeout(Duration::from_secs(15), client::connect(Self::config(), (connection.target.host.as_str(), connection.target.ssh_port), handler)).await {
                Ok(Ok(session)) => session,
                _ => {
                    if observed.lock().map_err(|_| "Ошибка SSH")?.as_ref().is_some_and(|k| k.fingerprint != connection.target.fingerprint) {
                        return Err("CP-SSH-HOSTKEY: Ключ SSH изменился. Пароль не отправлен; установка остановлена".into());
                    }
                    return Err("CP-SSH-CONNECT: Не удалось установить проверенное соединение SSH".into());
                }
            };
            let authentication = tokio::time::timeout(Duration::from_secs(20), session.authenticate_password(&connection.target.username, connection.password.as_str())).await;
            match authentication {
                Ok(Ok(result)) if result.success() => {},
                _ => { let _ = session.disconnect(Disconnect::ByApplication, "", "en").await; return Err("CP-SSH-AUTH: Вход SSH не выполнен. Проверьте root-пароль и разрешение password authentication".into()); }
            }
            let result = tokio::time::timeout(Duration::from_secs(if request["action"] == "install" { 900 } else { 60 }), async {
                let mut channel = session.channel_open_session().await.map_err(|_| "CP-SSH-CHANNEL: SSH-канал недоступен")?;
                // The command is constant and short. Bundled source and user JSON
                // share one length-framed stdin stream, never shell interpolation.
                let command = format!("python3 -c '{PYTHON_BOOTSTRAP}'");
                channel.exec(true, command).await.map_err(|_| "CP-SSH-EXEC: VPS не запустил контроллер Python 3")?;
                let input = controller_frame(CONTROLLER, request)?;
                channel.data(input.as_slice()).await.map_err(|_| "CP-SSH-WRITE: Не отправить запрос VPS")?;
                channel.eof().await.map_err(|_| "CP-SSH-WRITE: Не завершить запрос VPS")?;
                let mut output = Zeroizing::new(Vec::new());
                let mut stderr_size = 0usize;
                let mut exit = None;
                while let Some(message) = channel.wait().await {
                    match message {
                        ChannelMsg::Data { data } => {
                            if output.len() + data.len() > OUTPUT_LIMIT { let _ = channel.close().await; return Err("CP-SSH-LIMIT: Ответ VPS превышает 256 КБ".into()); }
                            output.extend_from_slice(&data);
                        },
                        ChannelMsg::ExtendedData { data, .. } => {
                            stderr_size += data.len();
                            if stderr_size > 32_768 { let _ = channel.close().await; return Err("CP-SSH-LIMIT: Слишком большой поток ошибок VPS".into()); }
                        },
                        ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
                        _ => {},
                    }
                }
                let value: Value = serde_json::from_slice(&output).map_err(|_| "CP-SERVER-RESPONSE: VPS не вернул JSON; проверьте наличие Python 3 и журнал сервера")?;
                if exit.is_none() { return Err("CP-SSH-CLOSED: Соединение закрылось до подтверждения результата. Проверьте сервер перед повтором".into()); }
                if exit != Some(0) && value["ok"] != false { return Err("CP-SERVER-EXIT: Контроллер VPS завершился с ошибкой".into()); }
                Ok(value)
            }).await;
            let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
            result.unwrap_or_else(|_| Err("CP-SSH-TIMEOUT: Время операции истекло. Удалённая установка могла продолжиться; проверьте VPS перед повтором".into()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::client::Handler;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PIN: &str = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const OTHER_PIN: &str = "SHA256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
    struct FakeTransport {
        calls: AtomicUsize,
        install_calls: AtomicUsize,
        blocked: bool,
    }
    impl Transport for FakeTransport {
        fn discover(&self, _: &str, _: u16) -> Result<HostKey, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(HostKey {
                fingerprint: PIN.into(),
                algorithm: "ssh-ed25519".into(),
            })
        }
        fn execute(&self, connection: &Connection, request: &Value) -> Result<Value, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(
                connection.password.as_str() == "test-password-only",
                "Fake transport received an unexpected test password"
            );
            assert!(request.get("password").is_none());
            let public = json!({"id":"client_1","name":"Телефон","created_at":"2026-10-21","revoked":false,"password":"never_public","uri":"never_public"});
            match request["action"].as_str().unwrap() {
                "inspect" => Ok(
                    json!({"ok":true,"ready":!self.blocked,"installed":false,"checks":[{"id":"ports","label":"Порты","ok":!self.blocked,"message":"Проверено"}],"blockers":if self.blocked {vec!["Порт занят"]}else{vec![]},"host":request["host"],"domain":request["domain"],"architecture":"x86_64","os":"Ubuntu 24.04"}),
                ),
                "install" => {
                    self.install_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(
                        json!({"ok":true,"installed":true,"service":"active","clients":[public],"password":"must_not_leak"}),
                    )
                }
                "clients.list" | "clients.revoke" => {
                    Ok(json!({"ok":true,"clients":[public],"service":"active"}))
                }
                "clients.export" | "clients.create" => Ok(
                    json!({"ok":true,"client":{"id":"client_1","name":"Телефон","created_at":"2026-10-21","revoked":false,"uri":format!("naive+https://test:temporary@{}:443#Phone",connection.target.domain)}}),
                ),
                _ => panic!("Unexpected action"),
            }
        }
    }
    fn setup(blocked: bool) -> (tempfile::TempDir, Manager, Arc<FakeTransport>) {
        let temp = tempfile::tempdir().unwrap();
        let transport = Arc::new(FakeTransport {
            calls: AtomicUsize::new(0),
            install_calls: AtomicUsize::new(0),
            blocked,
        });
        let manager =
            Manager::with_transport(temp.path().join("selfhost"), transport.clone()).unwrap();
        (temp, manager, transport)
    }
    fn credentials() -> Value {
        json!({"host":"203.0.113.20","ssh_port":22,"username":"root","password":"test-password-only","domain":"vpn.example.com","fingerprint":PIN})
    }
    fn inspect(manager: &Manager) -> Value {
        manager
            .request("POST", "/v1/servers/inspect", &credentials())
            .unwrap()
    }

    #[test]
    fn inventory_and_constructor_do_not_open_network() {
        let (_temp, manager, transport) = setup(false);
        assert_eq!(
            manager.request("GET", "/v1/servers", &json!({})).unwrap()["servers"],
            json!([])
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn rejects_injection_and_unknown_fields_before_network() {
        let (_temp, manager, transport) = setup(false);
        for host in [
            "example.com;id",
            "https://example.com",
            "example.com/evil",
            "localhost",
            "127.0.0.1",
            "::1",
            "-bad.example.com",
        ] {
            assert!(manager
                .request("POST", "/v1/servers/host-key", &json!({"host":host}))
                .is_err());
        }
        let mut body = credentials();
        body["command"] = json!("whoami");
        assert!(manager
            .request("POST", "/v1/servers/inspect", &body)
            .is_err());
        let mut body = credentials();
        body["fingerprint"] = json!("trust-all");
        assert!(manager
            .request("POST", "/v1/servers/inspect", &body)
            .is_err());
        let mut body = credentials();
        body["username"] = json!("admin;sudo");
        assert!(manager
            .request("POST", "/v1/servers/inspect", &body)
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn install_plan_is_bound_expiring_and_single_use() {
        let (_temp, manager, transport) = setup(false);
        let plan = inspect(&manager);
        let mut changed = credentials();
        changed["plan_id"] = plan["plan_id"].clone();
        changed["domain"] = json!("other.example.com");
        assert!(manager
            .request("POST", "/v1/servers/install", &changed)
            .is_err());
        assert_eq!(transport.install_calls.load(Ordering::SeqCst), 0);
        let mut good = credentials();
        good["plan_id"] = plan["plan_id"].clone();
        assert!(manager
            .request("POST", "/v1/servers/install", &good)
            .is_ok());
        assert!(manager
            .request("POST", "/v1/servers/install", &good)
            .is_err());
        assert_eq!(transport.install_calls.load(Ordering::SeqCst), 1);
        let fresh = inspect(&manager);
        manager
            .plans
            .lock()
            .unwrap()
            .iter_mut()
            .find(|p| p.id == fresh["plan_id"].as_str().unwrap())
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        good["plan_id"] = fresh["plan_id"].clone();
        assert!(manager
            .request("POST", "/v1/servers/install", &good)
            .is_err());
        assert_eq!(transport.install_calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn blockers_prevent_install_without_remote_mutation() {
        let (_temp, manager, transport) = setup(true);
        let plan = inspect(&manager);
        assert_eq!(plan["ready"], false);
        let mut body = credentials();
        body["plan_id"] = plan["plan_id"].clone();
        assert!(manager
            .request("POST", "/v1/servers/install", &body)
            .is_err());
        assert_eq!(transport.install_calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn persistence_and_public_results_never_include_credentials() {
        let (temp, manager, transport) = setup(false);
        let mut body = credentials();
        body["plan_id"] = inspect(&manager)["plan_id"].clone();
        let installed = manager
            .request("POST", "/v1/servers/install", &body)
            .unwrap();
        let serialized = serde_json::to_string(&installed).unwrap();
        assert!(!serialized.contains("password"));
        assert!(!serialized.contains("uri"));
        let disk = fs::read_to_string(temp.path().join("selfhost/servers.json")).unwrap();
        assert!(!disk.contains("password"));
        assert!(!disk.contains("temporary"));
        let reloaded = Manager::with_transport(temp.path().join("selfhost"), transport).unwrap();
        assert_eq!(
            reloaded.request("GET", "/v1/servers", &json!({})).unwrap()["servers"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let client_body =
            json!({"server_id":installed["server"]["id"],"password":"test-password-only"});
        let clients = manager
            .request("POST", "/v1/servers/clients/list", &client_body)
            .unwrap();
        assert!(!clients.to_string().contains("password"));
        assert!(!clients.to_string().contains("uri"));
        let mut export_body = client_body;
        export_body["client_id"] = json!("client_1");
        assert!(manager
            .request("POST", "/v1/servers/clients/export", &export_body)
            .unwrap()["client"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("naive+https://"));
    }
    #[test]
    fn saved_fingerprint_cannot_be_replaced_during_install() {
        let (_temp, manager, transport) = setup(false);
        let mut body = credentials();
        body["plan_id"] = inspect(&manager)["plan_id"].clone();
        manager
            .request("POST", "/v1/servers/install", &body)
            .unwrap();
        manager.servers.lock().unwrap()[0].target.fingerprint = OTHER_PIN.into();
        body["plan_id"] = inspect(&manager)["plan_id"].clone();
        assert!(manager
            .request("POST", "/v1/servers/install", &body)
            .is_err());
        assert_eq!(transport.install_calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn discovery_rejects_auth_and_pin_mismatch_rejects_key() {
        // A parseable fixed public-key fixture; no sockets, keys on disk or randomness needed.
        let public = russh::keys::PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .unwrap();
        let key: PublicKeyOrCertificate = public.into();
        let rt = SshTransport::runtime().unwrap();
        let observed = Arc::new(Mutex::new(None));
        let mut handler = KeyHandler {
            expected: None,
            observed: observed.clone(),
        };
        assert!(!rt.block_on(handler.check_server_key(&key)).unwrap());
        let actual = observed
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .fingerprint
            .clone();
        handler.expected = Some(OTHER_PIN.into());
        assert!(!rt.block_on(handler.check_server_key(&key)).unwrap());
        handler.expected = Some(actual);
        assert!(rt.block_on(handler.check_server_key(&key)).unwrap());
    }

    #[test]
    fn controller_error_rejects_unexpected_codes_and_redacts_complete_message() {
        let malicious = json!({"ok":false,"error":{"code":"test-password-only","message":"Rejected test-password-only\nrequest"}});
        let sanitized = controller_error(&malicious, "test-password-only");
        assert!(sanitized.starts_with("CP-SERVER-REMOTE:"));
        assert!(!sanitized.contains("test-password-only"));
        assert!(!sanitized.contains('\n'));
        let valid = json!({"ok":false,"error":{"code":"CP-SERVER-AUTH","message":"Access denied"}});
        assert!(controller_error(&valid, "other-password").starts_with("CP-SERVER-AUTH:"));
        // A password may itself resemble a syntactically valid controller code.
        let sanitized = controller_error(&valid, "CP-SERVER-AUTH");
        assert!(!sanitized.contains("CP-SERVER-AUTH"));
        assert!(sanitized.starts_with("[скрыто]:"));
    }

    #[test]
    fn source_and_request_use_one_exact_length_framed_stream() {
        let source = "# UTF-8 source: пример\nimport json\n";
        let request = json!({"action":"inspect","domain":"vpn.example.com","client_name":"value ' ; $(not-a-command)"});
        let frame = controller_frame(source, &request).unwrap();
        let newline = frame.iter().position(|b| *b == b'\n').unwrap();
        let count: usize = std::str::from_utf8(&frame[..newline])
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(count, source.len());
        assert!(frame[newline + 1..newline + 1 + count] == *source.as_bytes());
        let actual: Value = serde_json::from_slice(&frame[newline + 1 + count..]).unwrap();
        assert_eq!(actual, request);
        assert!(!PYTHON_BOOTSTRAP.contains('\''));
        assert!(PYTHON_BOOTSTRAP.len() < 512);
    }
}
