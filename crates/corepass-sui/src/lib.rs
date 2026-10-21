//! Existing S-UI panel integration. No startup network requests or tunnel control.
//! Only one fixed remote mutation exists: creating a Naive HTTPS client.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const MAX_RESPONSE: u64 = 2 * 1024 * 1024;
const MAX_PANELS: usize = 16;
const CREATE_WARNING: &str = "S-UI может перезапустить выбранный Naive-вход и прервать текущие подключения. Создавайте клиента после завершения игры.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Panel {
    id: String,
    name: String,
    base_url: String,
    server_domain: String,
    server_port: u16,
    #[serde(default = "read_only_default")]
    read_only: bool,
    created_at: String,
}
fn read_only_default() -> bool {
    true
}

impl Panel {
    fn public(&self) -> Value {
        let host = Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default();
        json!({"id":self.id,"name":self.name,"panel_host":host,"server_domain":self.server_domain,"server_port":self.server_port,"read_only":self.read_only,"created_at":self.created_at})
    }
}

#[derive(Clone, Copy)]
enum RemoteRequest<'a> {
    Inbounds(Option<u32>),
    Clients(Option<u32>),
    Create(&'a Value),
}
trait Transport: Send + Sync {
    fn request(
        &self,
        panel: &Panel,
        token: &str,
        request: RemoteRequest<'_>,
    ) -> Result<Value, String>;
}

pub struct Manager {
    data: PathBuf,
    panels: Mutex<Vec<Panel>>,
    mutation: Mutex<()>,
    transport: Arc<dyn Transport>,
}

struct SensitiveJson(Value);
impl Drop for SensitiveJson {
    fn drop(&mut self) {
        erase(&mut self.0);
    }
}
fn erase(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(erase),
        Value::Object(items) => items.values_mut().for_each(erase),
        _ => (),
    }
}

fn text<'a>(body: &'a Value, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Нужно поле {key}"))
}
fn clean(value: Option<&Value>, limit: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}
fn valid_local_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn remote_id(body: &Value, key: &str) -> Result<u32, String> {
    body.get(key)
        .and_then(Value::as_u64)
        .and_then(|id| u32::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| format!("Нужен положительный числовой {key}"))
}
fn domain_valid(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.is_ascii()
        && domain.contains('.')
        && domain.parse::<IpAddr>().is_err()
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
fn panel_url(input: &str) -> Result<Url, String> {
    if input.is_empty() || input.len() > 2048 || input.chars().any(char::is_control) {
        return Err("Нужен HTTPS-адрес панели длиной до 2048 байт".into());
    }
    let mut url = Url::parse(input).map_err(|_| "Некорректный HTTPS-адрес панели")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Адрес панели должен быть HTTPS без логина, пароля, query и fragment".into());
    }
    let host = url.host_str().unwrap();
    let unsafe_ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified() || ip.is_multicast(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified() || ip.is_multicast(),
        _ => false,
    };
    if host == "localhost" || host.ends_with(".localhost") || unsafe_ip {
        return Err("Нужен адрес внешней HTTPS-панели".into());
    }
    if url.path().trim_end_matches('/').ends_with("/apiv2") {
        return Err("Укажите адрес самой панели, без /apiv2".into());
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

pub fn validate_request(method: &str, path: &str, body: &Value) -> Result<(), String> {
    let object = body.as_object().ok_or("Нужен JSON-объект")?;
    let allowed: &[&str] = match (method, path) {
        ("GET", "/v1/sui/panels") => &[],
        ("POST", "/v1/sui/panels/save") => &[
            "name",
            "base_url",
            "server_domain",
            "server_port",
            "panel_id",
            "read_only",
        ],
        ("POST", "/v1/sui/inspect" | "/v1/sui/clients/list") => &["panel_id", "token"],
        ("POST", "/v1/sui/clients/export") => &["panel_id", "token", "client_id", "inbound_id"],
        ("POST", "/v1/sui/clients/create") => &[
            "panel_id",
            "token",
            "inbound_id",
            "client_name",
            "confirm_create",
        ],
        _ => return Err("Маршрут S-UI не найден".into()),
    };
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Неизвестные параметры команды S-UI".into());
    }
    if path == "/v1/sui/panels" {
        return Ok(());
    }
    if path == "/v1/sui/panels/save" {
        let update = body.get("panel_id").is_some();
        if !update || body.get("name").is_some() {
            let name = text(body, "name")?;
            if name.trim().is_empty()
                || name.chars().count() > 80
                || name.chars().any(char::is_control)
            {
                return Err("Введите название панели до 80 символов без управляющих знаков".into());
            }
        }
        let url = if !update || body.get("base_url").is_some() {
            Some(panel_url(text(body, "base_url")?)?)
        } else {
            None
        };
        let domain = body
            .get("server_domain")
            .map(|_| text(body, "server_domain"))
            .transpose()?;
        let domain = domain.or_else(|| url.as_ref().and_then(Url::host_str));
        if (!update || body.get("server_domain").is_some())
            && !domain.is_some_and(|d| domain_valid(&d.to_ascii_lowercase()))
        {
            return Err(
                "Укажите публичный домен VPN для нормальной проверки HTTPS-сертификата".into(),
            );
        }
        if let Some(port) = body.get("server_port") {
            if !port.as_u64().is_some_and(|p| p > 0 && p <= u16::MAX as u64) {
                return Err("Некорректный публичный порт VPN".into());
            }
        }
        if let Some(id) = body.get("panel_id") {
            if !id.as_str().is_some_and(valid_local_id) {
                return Err("Некорректный panel_id".into());
            }
        }
        if let Some(value) = body.get("read_only") {
            if !value.is_boolean() {
                return Err("read_only должен быть логическим значением".into());
            }
        }
        return Ok(());
    }
    if !valid_local_id(text(body, "panel_id")?) {
        return Err("Некорректный panel_id".into());
    }
    let token = text(body, "token")?;
    if token.is_empty() || token.len() > 2048 || !token.bytes().all(|b| b >= 0x21 && b <= 0x7e) {
        return Err("Нужен API-токен S-UI без пробелов и управляющих знаков".into());
    }
    if path.ends_with("/create") || path.ends_with("/export") {
        remote_id(body, "inbound_id")?;
    }
    if path.ends_with("/export") {
        remote_id(body, "client_id")?;
    }
    if path.ends_with("/create") {
        let name = text(body, "client_name")?;
        if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control)
        {
            return Err("Имя клиента: от 1 до 64 символов без управляющих знаков".into());
        }
        if body["confirm_create"] != true {
            return Err(CREATE_WARNING.into());
        }
    }
    Ok(())
}

impl Manager {
    pub fn new(data: PathBuf) -> Result<Self, String> {
        Self::with_transport(data, Arc::new(HttpsTransport))
    }
    fn with_transport(data: PathBuf, transport: Arc<dyn Transport>) -> Result<Self, String> {
        fs::create_dir_all(&data).map_err(|_| "Не создать каталог S-UI")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&data, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Не защитить каталог S-UI")?;
        }
        let file = data.join("panels.json");
        let panels: Vec<Panel> = if file.exists() {
            let bytes = fs::read(&file).map_err(|_| "Не читается список панелей")?;
            if bytes.len() > 131_072 {
                return Err("Список панелей повреждён".into());
            }
            serde_json::from_slice(&bytes).map_err(|_| "Повреждён список панелей")?
        } else {
            vec![]
        };
        if panels.len() > MAX_PANELS {
            return Err("Слишком много панелей S-UI".into());
        }
        for panel in &panels {
            if !valid_local_id(&panel.id)
                || panel_url(&panel.base_url).is_err()
                || !domain_valid(&panel.server_domain)
                || panel.server_port == 0
            {
                return Err("Повреждена запись панели S-UI".into());
            }
        }
        Ok(Self {
            data,
            panels: Mutex::new(panels),
            mutation: Mutex::new(()),
            transport,
        })
    }
    pub fn request(&self, method: &str, path: &str, body: &Value) -> Result<Value, String> {
        validate_request(method, path, body)?;
        if method == "GET" {
            return Ok(
                json!({"panels":self.panels.lock().map_err(|_| "Ошибка панелей")?.iter().map(Panel::public).collect::<Vec<_>>(),"supported":true}),
            );
        }
        let _operation = self
            .mutation
            .try_lock()
            .map_err(|_| "Другая операция S-UI ещё выполняется")?;
        if path == "/v1/sui/panels/save" {
            return self.save(body);
        }
        let panel = self
            .panels
            .lock()
            .map_err(|_| "Ошибка панелей")?
            .iter()
            .find(|p| p.id == body["panel_id"])
            .cloned()
            .ok_or("Панель не найдена")?;
        if path == "/v1/sui/clients/create" && panel.read_only {
            return Err("Панель сохранена только для чтения. Сначала явно разрешите создание клиентов в её настройках".into());
        }
        let token = Zeroizing::new(text(body, "token")?.to_owned());
        let result = match path {
            "/v1/sui/inspect" => {
                let inbounds = self.fetch(&panel, &token, RemoteRequest::Inbounds(None))?;
                Ok(
                    json!({"panel":panel.public(),"inbounds":public_inbounds(&inbounds.0)?,"warning":CREATE_WARNING,"endpoint_verified":false}),
                )
            }
            "/v1/sui/clients/list" => {
                let clients = self.fetch(&panel, &token, RemoteRequest::Clients(None))?;
                let inbounds = self.fetch(&panel, &token, RemoteRequest::Inbounds(None))?;
                let naive_ids = naive_ids(&inbounds.0)?;
                Ok(
                    json!({"clients":array(&clients.0,"clients")?.iter().filter(|c| ids(c.get("inbounds")).iter().any(|id| naive_ids.contains(id))).map(public_client).collect::<Result<Vec<_>,_>>()?,"inbounds":public_inbounds(&inbounds.0)?,"read_only":panel.read_only,"warning":CREATE_WARNING}),
                )
            }
            "/v1/sui/clients/export" => {
                let client_id = remote_id(body, "client_id")?;
                let inbound_id = remote_id(body, "inbound_id")?;
                self.check_inbound(&panel, &token, inbound_id)?;
                let clients =
                    self.fetch(&panel, &token, RemoteRequest::Clients(Some(client_id)))?;
                let client = array(&clients.0, "clients")?
                    .iter()
                    .find(|c| c["id"] == client_id)
                    .ok_or("Клиент не найден")?;
                if client["enable"] != true || !ids(client.get("inbounds")).contains(&inbound_id) {
                    return Err("Клиент выключен или не привязан к выбранному Naive-входу".into());
                }
                let config = json_field(client.get("config"))?;
                let username = config["naive"]["username"]
                    .as_str()
                    .ok_or("В конфигурации клиента нет Naive username")?;
                let password = config["naive"]["password"]
                    .as_str()
                    .ok_or("В конфигурации клиента нет Naive password")?;
                let name = display_name(client);
                Ok(
                    json!({"client":{"id":client_id,"name":name,"uri":make_uri(&panel,username,password,&name)?},"endpoint_verified":false}),
                )
            }
            "/v1/sui/clients/create" => self.create(&panel, &token, body),
            _ => Err("Маршрут S-UI не найден".into()),
        };
        result.map_err(|e: String| {
            e.replace(token.as_str(), "[скрыто]")
                .replace(&panel.base_url, "[адрес панели]")
        })
    }
    fn save(&self, body: &Value) -> Result<Value, String> {
        let mut panels = self.panels.lock().map_err(|_| "Ошибка списка панелей")?;
        let mut updated = panels.clone();
        let existing = body
            .get("panel_id")
            .and_then(Value::as_str)
            .map(|id| {
                updated
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or("Панель не найдена")
            })
            .transpose()?;
        if existing.is_none() && updated.len() >= MAX_PANELS {
            return Err("Достигнут предел 16 панелей".into());
        }
        let before = existing.map(|index| &updated[index]);
        let url = panel_url(
            body.get("base_url")
                .and_then(Value::as_str)
                .or_else(|| before.map(|p| p.base_url.as_str()))
                .ok_or("Нужен адрес новой панели")?,
        )?;
        let domain = body
            .get("server_domain")
            .and_then(Value::as_str)
            .or_else(|| before.map(|p| p.server_domain.as_str()))
            .unwrap_or(url.host_str().unwrap())
            .to_ascii_lowercase();
        let server_port = body
            .get("server_port")
            .and_then(Value::as_u64)
            .map(|p| p as u16)
            .or_else(|| before.map(|p| p.server_port))
            .unwrap_or(443);
        let endpoint_changed = before.is_some_and(|p| {
            p.base_url != url.as_str() || p.server_domain != domain || p.server_port != server_port
        });
        let panel = Panel {
            id: existing
                .map(|i| updated[i].id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
            name: body
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| before.map(|p| p.name.as_str()))
                .ok_or("Нужно название панели")?
                .trim()
                .into(),
            base_url: url.to_string(),
            server_domain: domain,
            server_port,
            read_only: body
                .get("read_only")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    if endpoint_changed {
                        true
                    } else {
                        before.map_or(true, |p| p.read_only)
                    }
                }),
            created_at: existing
                .map(|i| updated[i].created_at.clone())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        };
        if let Some(i) = existing {
            updated[i] = panel.clone();
        } else {
            updated.push(panel.clone());
        }
        atomic_store(&self.data.join("panels.json"), &updated)?;
        *panels = updated;
        Ok(json!({"panel":panel.public()}))
    }
    fn fetch(
        &self,
        panel: &Panel,
        token: &str,
        request: RemoteRequest<'_>,
    ) -> Result<SensitiveJson, String> {
        let raw = SensitiveJson(self.transport.request(panel, token, request)?);
        if raw.0["success"] != true {
            return Err("CP-SUI-REJECTED: Панель отклонила запрос. Проверьте токен и права доступа; повторная запись автоматически не выполняется".into());
        }
        Ok(raw)
    }
    fn check_inbound(&self, panel: &Panel, token: &str, id: u32) -> Result<(), String> {
        let inbounds = self.fetch(panel, token, RemoteRequest::Inbounds(Some(id)))?;
        let inbound = array(&inbounds.0, "inbounds")?
            .iter()
            .find(|inbound| inbound["id"] == id)
            .ok_or("Вход не найден")?;
        if inbound["type"] != "naive" || inbound["network"] == "udp" {
            return Err(
                "Поддерживается только Naive через TCP/HTTPS; другие входы не изменяются".into(),
            );
        }
        if !endpoint_supported(panel, inbound)? {
            return Err("CP-SUI-ENDPOINT: Публичный HTTPS-адрес не совпадает с безопасным addrs/out_json выбранного Naive-входа. Проверьте домен и внешний порт; внутренний listen_port не используется".into());
        }
        Ok(())
    }
    fn create(&self, panel: &Panel, token: &str, body: &Value) -> Result<Value, String> {
        let inbound_id = remote_id(body, "inbound_id")?;
        self.check_inbound(panel, token, inbound_id)?;
        let sub_id = Zeroizing::new(uuid::Uuid::new_v4().simple().to_string());
        let username = Zeroizing::new(uuid::Uuid::new_v4().simple().to_string());
        let password = Zeroizing::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let label = text(body, "client_name")?.trim();
        let data = SensitiveJson(
            json!({"id":0,"enable":true,"name":sub_id.as_str(),"remark":label,"desc":"Created by Corepass","group":"","config":{"naive":{"username":username.as_str(),"password":password.as_str()}},"inbounds":[inbound_id],"links":[],"volume":0,"expiry":0,"up":0,"down":0,"totalUp":0,"totalDown":0,"delayStart":false,"autoReset":false,"resetDays":0,"nextReset":0}),
        );
        // Exactly one POST. A timeout/failure is never retried automatically.
        self.fetch(panel,token,RemoteRequest::Create(&data.0)).map_err(|_| "CP-SUI-CREATE-UNKNOWN: Запись не подтверждена. Клиент мог быть создан; проверьте список в панели перед повтором".to_owned())?;
        let clients = self.fetch(panel,token,RemoteRequest::Clients(None)).map_err(|_| "CP-SUI-CREATE-UNKNOWN: Панель приняла запись, но список клиентов не получен. Не повторяйте создание без проверки".to_owned())?;
        let client = array(&clients.0,"clients").map_err(|_| "CP-SUI-CREATE-UNKNOWN: Панель приняла запись, но вернула неполный список. Не повторяйте создание без проверки".to_owned())?.iter().find(|c| c["name"].as_str() == Some(sub_id.as_str())).ok_or("CP-SUI-CREATE-UNKNOWN: Клиент не найден после записи; проверьте панель перед повтором")?;
        let id = client["id"]
            .as_u64()
            .filter(|id| *id > 0 && *id <= u32::MAX as u64)
            .ok_or("CP-SUI-CREATE-UNKNOWN: Панель не подтвердила идентификатор клиента")?;
        let verified = self.fetch(panel,token,RemoteRequest::Clients(Some(id as u32))).map_err(|_| "CP-SUI-CREATE-UNKNOWN: Панель приняла запись, но полная конфигурация не прочитана. Не повторяйте создание без проверки".to_owned())?;
        let persisted = array(&verified.0,"clients").map_err(|_| "CP-SUI-CREATE-UNKNOWN: Не подтверждена полная запись созданного клиента; проверьте панель перед повтором".to_owned())?.iter().find(|client|client["id"]==id).ok_or("CP-SUI-CREATE-UNKNOWN: Сохранённый клиент не найден; проверьте панель перед повтором")?;
        let persisted_config = json_field(persisted.get("config")).map_err(|_| {
            "CP-SUI-CREATE-UNKNOWN: Панель не подтвердила конфигурацию созданного клиента"
                .to_owned()
        })?;
        if persisted["enable"] != true
            || !ids(persisted.get("inbounds")).contains(&inbound_id)
            || persisted_config["naive"]["username"].as_str() != Some(username.as_str())
            || persisted_config["naive"]["password"].as_str() != Some(password.as_str())
        {
            return Err("CP-SUI-CREATE-UNKNOWN: Сохранённые данные не совпали с запросом. QR-код не выдан; проверьте клиента в панели и не повторяйте создание автоматически".into());
        }
        Ok(
            json!({"client":{"id":id,"name":label,"uri":make_uri(panel,&username,&password,label)?},"warning":CREATE_WARNING,"endpoint_verified":false}),
        )
    }
}

fn endpoint_supported(panel: &Panel, inbound: &Value) -> Result<bool, String> {
    let outbound = json_field(inbound.get("out_json"))?;
    if outbound["type"]
        .as_str()
        .is_some_and(|kind| kind != "naive")
        || outbound["quic"] == true
    {
        return Ok(false);
    }
    let addrs = inbound
        .get("addrs")
        .map(|value| json_field(Some(value)))
        .transpose()?;
    let list = addrs
        .as_ref()
        .and_then(|value| value.as_array())
        .filter(|list| !list.is_empty());
    let base_tls = outbound
        .get("tls")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let candidates = list.cloned().unwrap_or_else(|| vec![outbound.0.clone()]);
    if candidates.len() > 64 {
        return Err("CP-SUI-LIMIT: Слишком много публичных адресов входа".into());
    }
    for candidate in candidates {
        if candidate["server"].as_str() != Some(panel.server_domain.as_str())
            || candidate["server_port"].as_u64() != Some(panel.server_port as u64)
        {
            continue;
        }
        let mut tls = base_tls.clone();
        if let Some(overrides) = candidate.get("tls").and_then(Value::as_object) {
            tls.extend(overrides.clone());
        }
        if tls.get("enabled").and_then(Value::as_bool) != Some(true)
            || tls.get("insecure").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        if tls
            .get("server_name")
            .and_then(Value::as_str)
            .is_some_and(|name| name != panel.server_domain)
        {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn json_field(value: Option<&Value>) -> Result<SensitiveJson, String> {
    let value = value.ok_or("В ответе нет требуемой конфигурации")?;
    Ok(SensitiveJson(if let Some(text) = value.as_str() {
        serde_json::from_str(text).map_err(|_| "Некорректный JSON конфигурации")?
    } else {
        value.clone()
    }))
}
impl std::ops::Deref for SensitiveJson {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
fn array<'a>(envelope: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    let items = envelope["obj"][key]
        .as_array()
        .ok_or("CP-SUI-SCHEMA: Панель вернула неподдерживаемую структуру ответа")?;
    if items.len() > 2048 {
        return Err("CP-SUI-LIMIT: Слишком много объектов в ответе панели".into());
    }
    Ok(items)
}
fn ids(value: Option<&Value>) -> Vec<u32> {
    let parsed = value.and_then(|v| {
        if v.is_string() {
            serde_json::from_str::<Value>(v.as_str()?).ok()
        } else {
            Some(v.clone())
        }
    });
    parsed
        .and_then(|v| {
            v.as_array().map(|ids| {
                ids.iter()
                    .filter_map(|id| {
                        id.as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .filter(|n| *n > 0)
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}
fn display_name(client: &Value) -> String {
    let remark = clean(client.get("remark"), 80);
    if remark.trim().is_empty() {
        format!("Клиент {}", client["id"].as_u64().unwrap_or(0))
    } else {
        remark
    }
}
fn public_client(client: &Value) -> Result<Value, String> {
    let id = client["id"]
        .as_u64()
        .filter(|id| *id > 0 && *id <= u32::MAX as u64)
        .ok_or("CP-SUI-SCHEMA: Некорректный клиент")?;
    // S-UI's name is a subscription secret. Only its display remark leaves here.
    Ok(
        json!({"id":id,"name":display_name(client),"enable":client["enable"].as_bool().unwrap_or(false),"inbounds":ids(client.get("inbounds")),"expiry":client["expiry"].as_i64().unwrap_or(0),"volume":client["volume"].as_i64().unwrap_or(0),"up":client["up"].as_i64().unwrap_or(0),"down":client["down"].as_i64().unwrap_or(0)}),
    )
}
fn naive_ids(envelope: &Value) -> Result<Vec<u32>, String> {
    Ok(array(envelope, "inbounds")?
        .iter()
        .filter(|i| i["type"] == "naive")
        .filter_map(|i| {
            i["id"]
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id > 0)
        })
        .collect())
}
fn public_inbounds(envelope: &Value) -> Result<Vec<Value>, String> {
    array(envelope,"inbounds")?.iter().filter(|i| i["type"] == "naive").map(|i| {
        let id = i["id"].as_u64().filter(|id| *id > 0 && *id <= u32::MAX as u64).ok_or("CP-SUI-SCHEMA: Некорректный вход")?;
        Ok(json!({"id":id,"tag":clean(i.get("tag"),80),"type":"naive","listen_port":i["listen_port"].as_u64().unwrap_or(0),"client_count":i["users"].as_array().map_or(0,Vec::len)}))
    }).collect()
}
fn make_uri(panel: &Panel, username: &str, password: &str, name: &str) -> Result<String, String> {
    if [username, password]
        .iter()
        .any(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err("CP-SUI-CREDENTIALS: Некорректные Naive-данные клиента".into());
    }
    let mut uri = Url::parse(&format!(
        "naive+https://{}:{}/",
        panel.server_domain, panel.server_port
    ))
    .map_err(|_| "Некорректный адрес VPN")?;
    uri.set_username(username)
        .map_err(|_| "Ошибка ссылки клиента")?;
    uri.set_password(Some(password))
        .map_err(|_| "Ошибка ссылки клиента")?;
    uri.set_path("");
    uri.set_fragment(Some(name));
    Ok(uri.to_string())
}

fn atomic_store(path: &Path, panels: &[Panel]) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(panels).map_err(|_| "Ошибка сохранения панелей")?,
    )
    .map_err(|_| "Не записать список панелей")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(|_| "Не защитить список панелей")?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            unsafe extern "system" {
                fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
            }
            if MoveFileExW(a.as_ptr(), b.as_ptr(), 9) == 0 {
                return Err("Не заменить список панелей атомарно".into());
            }
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temporary, path).map_err(|_| "Не заменить список панелей атомарно")?;
    Ok(())
}

struct HttpsTransport;
impl Transport for HttpsTransport {
    fn request(
        &self,
        panel: &Panel,
        token: &str,
        request: RemoteRequest<'_>,
    ) -> Result<Value, String> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| "CP-SUI-HTTP: Не создать HTTPS-клиент")?;
        let base = panel_url(&panel.base_url)?;
        let (endpoint, id) = match request {
            RemoteRequest::Inbounds(id) => ("inbounds", id),
            RemoteRequest::Clients(id) => ("clients", id),
            RemoteRequest::Create(_) => ("save", None),
        };
        let mut url = base
            .join(&format!("apiv2/{endpoint}"))
            .map_err(|_| "CP-SUI-URL: Некорректный адрес API")?;
        if let Some(id) = id {
            url.query_pairs_mut().append_pair("id", &id.to_string());
        }
        let mut header = reqwest::header::HeaderValue::from_str(token)
            .map_err(|_| "CP-SUI-TOKEN: Некорректный API-токен")?;
        header.set_sensitive(true);
        let builder = match request {
            RemoteRequest::Create(data) => {
                let serialized = Zeroizing::new(
                    serde_json::to_string(data).map_err(|_| "CP-SUI-JSON: Ошибка запроса")?,
                );
                let encoded = Zeroizing::new(
                    url::form_urlencoded::Serializer::new(String::new())
                        .append_pair("object", "clients")
                        .append_pair("action", "new")
                        .append_pair("data", &serialized)
                        .finish(),
                );
                client
                    .post(url)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(encoded.as_bytes().to_vec())
            }
            _ => client.get(url),
        };
        let response = builder.header("Token",header).header(reqwest::header::ACCEPT,"application/json").send().map_err(|_| "CP-SUI-TRANSPORT: HTTPS-запрос не завершился. Для записи результат может быть неопределён; автоматического повтора нет")?;
        if !response.status().is_success() {
            return Err("CP-SUI-HTTP: Панель вернула ошибку HTTP; перенаправления и отключение проверки TLS запрещены".into());
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE)
        {
            return Err("CP-SUI-LIMIT: Ответ панели превышает 2 МБ".into());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "CP-SUI-READ: Не прочитать ответ панели")?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err("CP-SUI-LIMIT: Ответ панели превышает 2 МБ".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "CP-SUI-JSON: Панель не вернула JSON".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FakeTransport {
        calls: AtomicUsize,
        writes: AtomicUsize,
        fail_envelope: AtomicBool,
        fail_post: AtomicBool,
        corrupt_readback: AtomicBool,
        clients: Mutex<Vec<Value>>,
        created: Mutex<Option<Value>>,
    }
    fn client() -> Value {
        json!({"id":3,"enable":true,"name":"secret-subscription-id","remark":"Мой телефон","inbounds":[7],"up":1,"down":2,"volume":0,"expiry":0,"config":{"naive":{"username":"naive:a@b","password":"p@ss:/%"}},"links":[{"uri":"malicious-external-link"}]})
    }
    fn inbound(id: u32) -> Value {
        if id == 7 {
            json!({"id":7,"type":"naive","tag":"Finland","listen_port":1443,"listen":"127.0.0.1","network":"tcp","users":["secret-subscription-id"],"out_json":{"server":"vpn.example.com","server_port":443,"tls":{"enabled":true}}})
        } else {
            json!({"id":9,"type":"vless","tag":"Other protocol","listen_port":8443})
        }
    }
    impl Transport for FakeTransport {
        fn request(
            &self,
            _panel: &Panel,
            token: &str,
            request: RemoteRequest<'_>,
        ) -> Result<Value, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(
                token == "fixture-token",
                "Fake transport received unexpected test token"
            );
            if self.fail_envelope.load(Ordering::SeqCst) {
                return Ok(
                    json!({"success":false,"msg":"fixture-token secret-panel-path raw HTML","obj":null}),
                );
            }
            match request {
                RemoteRequest::Inbounds(id) => Ok(
                    json!({"success":true,"obj":{"inbounds":id.map(|id|vec![inbound(id)]).unwrap_or_else(||vec![inbound(7),inbound(9)])}}),
                ),
                RemoteRequest::Clients(id) => {
                    let clients = self.clients.lock().unwrap();
                    let result = clients
                        .iter()
                        .filter(|client| id.map_or(true, |id| client["id"] == id))
                        .cloned()
                        .collect::<Vec<_>>();
                    Ok(json!({"success":true,"obj":{"clients":result}}))
                }
                RemoteRequest::Create(data) => {
                    self.writes.fetch_add(1, Ordering::SeqCst);
                    if self.fail_post.load(Ordering::SeqCst) {
                        return Err("Simulated ambiguous request outcome".into());
                    }
                    *self.created.lock().unwrap() = Some(data.clone());
                    let mut created = data.clone();
                    created["id"] = json!(42);
                    if self.corrupt_readback.load(Ordering::SeqCst) {
                        created["config"]["naive"]["password"] = json!("changed-by-panel");
                    }
                    self.clients.lock().unwrap().push(created);
                    Ok(json!({"success":true,"msg":"save","obj":{}}))
                }
            }
        }
    }
    fn setup() -> (tempfile::TempDir, Manager, Arc<FakeTransport>) {
        let temporary = tempfile::tempdir().unwrap();
        let transport = Arc::new(FakeTransport {
            calls: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            fail_envelope: AtomicBool::new(false),
            fail_post: AtomicBool::new(false),
            corrupt_readback: AtomicBool::new(false),
            clients: Mutex::new(vec![client()]),
            created: Mutex::new(None),
        });
        let manager =
            Manager::with_transport(temporary.path().join("sui"), transport.clone()).unwrap();
        (temporary, manager, transport)
    }
    fn save(manager: &Manager) -> Value {
        manager.request("POST","/v1/sui/panels/save",&json!({"name":"My panel","base_url":"https://panel.example.com/secret-panel-path/","server_domain":"vpn.example.com","server_port":443})).unwrap()["panel"].clone()
    }
    fn token(panel: &Value) -> Value {
        json!({"panel_id":panel["id"],"token":"fixture-token"})
    }

    #[test]
    fn local_inventory_and_save_do_not_open_network_or_persist_token() {
        let (temporary, manager, transport) = setup();
        assert_eq!(
            manager
                .request("GET", "/v1/sui/panels", &json!({}))
                .unwrap()["panels"],
            json!([])
        );
        let panel = save(&manager);
        assert_eq!(panel["read_only"], true);
        assert!(!panel.to_string().contains("secret-panel-path"));
        assert!(panel.get("base_url").is_none());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        let disk = fs::read_to_string(temporary.path().join("sui/panels.json")).unwrap();
        assert!(!disk.contains("fixture-token"));
        assert!(!disk.contains("password"));
        let restored = Manager::with_transport(temporary.path().join("sui"), transport).unwrap();
        assert_eq!(
            restored
                .request("GET", "/v1/sui/panels", &json!({}))
                .unwrap()["panels"][0]["id"],
            panel["id"]
        );
    }
    #[test]
    fn schema_rejects_unknown_commands_and_non_https_before_transport() {
        let (_temporary, manager, transport) = setup();
        for url in [
            "http://panel.example.com/app/",
            "https://user:pass@panel.example.com/",
            "https://panel.example.com/?token=secret",
            "https://127.0.0.1/app/",
            "https://[::1]/app/",
            "https://panel.example.com/app/apiv2/",
        ] {
            assert!(manager
                .request(
                    "POST",
                    "/v1/sui/panels/save",
                    &json!({"name":"Panel","base_url":url})
                )
                .is_err());
        }
        assert!(manager
            .request("POST", "/v1/sui/restart", &json!({}))
            .is_err());
        let panel = save(&manager);
        let mut body = token(&panel);
        body["command"] = json!("anything");
        assert!(manager.request("POST", "/v1/sui/inspect", &body).is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn create_requires_local_write_policy_and_confirmation_before_any_http() {
        let (_temporary, manager, transport) = setup();
        let panel = save(&manager);
        let mut body = token(&panel);
        body["inbound_id"] = json!(7);
        body["client_name"] = json!("New phone");
        body["confirm_create"] = json!(true);
        assert!(manager
            .request("POST", "/v1/sui/clients/create", &body)
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        manager
            .request(
                "POST",
                "/v1/sui/panels/save",
                &json!({"panel_id":panel["id"],"read_only":false}),
            )
            .unwrap();
        body["confirm_create"] = json!(false);
        assert!(manager
            .request("POST", "/v1/sui/clients/create", &body)
            .is_err());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        assert_eq!(transport.writes.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn partial_policy_update_preserves_private_base_and_endpoint_change_resets_policy() {
        let (_temporary, manager, _transport) = setup();
        let panel = save(&manager);
        let writable = manager
            .request(
                "POST",
                "/v1/sui/panels/save",
                &json!({"panel_id":panel["id"],"read_only":false}),
            )
            .unwrap();
        assert_eq!(writable["panel"]["read_only"], false);
        assert!(manager.panels.lock().unwrap()[0]
            .base_url
            .ends_with("/secret-panel-path/"));
        let reset = manager
            .request(
                "POST",
                "/v1/sui/panels/save",
                &json!({"panel_id":panel["id"],"server_domain":"other.example.com"}),
            )
            .unwrap();
        assert_eq!(reset["panel"]["read_only"], true);
    }
    #[test]
    fn http_200_failure_envelope_is_rejected_without_raw_error_or_secrets() {
        let (_temporary, manager, transport) = setup();
        let panel = save(&manager);
        transport.fail_envelope.store(true, Ordering::SeqCst);
        let error = manager
            .request("POST", "/v1/sui/inspect", &token(&panel))
            .unwrap_err();
        assert!(error.starts_with("CP-SUI-REJECTED"));
        assert!(!error.contains("fixture-token"));
        assert!(!error.contains("secret-panel-path"));
        assert!(!error.contains("HTML"));
    }
    #[test]
    fn public_clients_exclude_subscription_ids_and_credentials() {
        let (_temporary, manager, transport) = setup();
        let panel = save(&manager);
        let listed = manager
            .request("POST", "/v1/sui/clients/list", &token(&panel))
            .unwrap();
        assert_eq!(listed["clients"][0]["name"], "Мой телефон");
        let encoded = listed.to_string();
        for secret in [
            "secret-subscription-id",
            "naive:a@b",
            "p@ss:/%",
            "malicious-external-link",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(listed["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(transport.writes.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn export_uses_public_https_endpoint_not_internal_listen_port_or_panel_links() {
        let (_temporary, manager, transport) = setup();
        let panel = save(&manager);
        let mut body = token(&panel);
        body["client_id"] = json!(3);
        body["inbound_id"] = json!(7);
        let result = manager
            .request("POST", "/v1/sui/clients/export", &body)
            .unwrap();
        let uri = Url::parse(result["client"]["uri"].as_str().unwrap()).unwrap();
        assert_eq!(uri.scheme(), "naive+https");
        assert_eq!(uri.host_str(), Some("vpn.example.com"));
        assert_eq!(uri.port(), Some(443));
        assert!(!uri.to_string().contains(":1443"));
        assert!(uri.to_string().contains("naive%3Aa%40b"));
        assert_eq!(transport.writes.load(Ordering::SeqCst), 0);
        body["inbound_id"] = json!(9);
        assert!(manager
            .request("POST", "/v1/sui/clients/export", &body)
            .is_err());
    }
    #[test]
    fn create_generates_random_credentials_and_changes_only_chosen_naive_inbound() {
        let (_temporary, manager, transport) = setup();
        let panel = save(&manager);
        manager
            .request(
                "POST",
                "/v1/sui/panels/save",
                &json!({"panel_id":panel["id"],"read_only":false}),
            )
            .unwrap();
        let original = transport.clients.lock().unwrap()[0].clone();
        let mut body = token(&panel);
        body["inbound_id"] = json!(7);
        body["client_name"] = json!("New phone");
        body["confirm_create"] = json!(true);
        let result = manager
            .request("POST", "/v1/sui/clients/create", &body)
            .unwrap();
        let created = transport.created.lock().unwrap();
        let created = created.as_ref().unwrap();
        assert_eq!(created["inbounds"], json!([7]));
        assert_eq!(created["config"].as_object().unwrap().len(), 1);
        assert_eq!(created["config"]["naive"].as_object().unwrap().len(), 2);
        assert_eq!(
            created["config"]["naive"]["password"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(created["name"].as_str().unwrap().len(), 32);
        assert_ne!(created["name"], created["config"]["naive"]["username"]);
        assert_eq!(transport.clients.lock().unwrap()[0], original);
        assert_eq!(transport.writes.load(Ordering::SeqCst), 1);
        assert_eq!(result["client"]["id"], 42);
        assert_eq!(result["client"]["name"], "New phone");
        assert!(result["warning"]
            .as_str()
            .unwrap()
            .contains("перезапустить"));
    }

    #[test]
    fn ambiguous_create_and_mismatched_readback_are_not_retried_or_exported() {
        for corrupt in [false, true] {
            let (_temporary, manager, transport) = setup();
            let panel = save(&manager);
            manager
                .request(
                    "POST",
                    "/v1/sui/panels/save",
                    &json!({"panel_id":panel["id"],"read_only":false}),
                )
                .unwrap();
            if corrupt {
                transport.corrupt_readback.store(true, Ordering::SeqCst);
            } else {
                transport.fail_post.store(true, Ordering::SeqCst);
            }
            let mut body = token(&panel);
            body["inbound_id"] = json!(7);
            body["client_name"] = json!("New phone");
            body["confirm_create"] = json!(true);
            let error = manager
                .request("POST", "/v1/sui/clients/create", &body)
                .unwrap_err();
            assert!(error.starts_with("CP-SUI-CREATE-UNKNOWN"));
            assert!(!error.contains("naive+https"));
            assert_eq!(transport.writes.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn public_endpoint_requires_safe_matching_outbound_tls_and_rejects_quic() {
        let (_temporary, manager, _transport) = setup();
        save(&manager);
        let panel = manager.panels.lock().unwrap()[0].clone();
        let mut config = inbound(7);
        assert!(endpoint_supported(&panel, &config).unwrap());
        config["out_json"]["tls"]["insecure"] = json!(true);
        assert!(!endpoint_supported(&panel, &config).unwrap());
        config = inbound(7);
        config["out_json"]["quic"] = json!(true);
        assert!(!endpoint_supported(&panel, &config).unwrap());
        config = inbound(7);
        config["out_json"]["server_port"] = json!(1443);
        assert!(!endpoint_supported(&panel, &config).unwrap());
        config["addrs"] = json!([{"server":"vpn.example.com","server_port":443,"tls":{"enabled":true,"server_name":"vpn.example.com"}}]);
        assert!(endpoint_supported(&panel, &config).unwrap());
        config["addrs"][0]["tls"]["server_name"] = json!("wrong.example.com");
        assert!(!endpoint_supported(&panel, &config).unwrap());
    }
}
