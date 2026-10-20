use base64::{engine::general_purpose, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

pub const API_PORT: u16 = 17871;
pub const PROXY_PORT: u16 = 17872;
pub const METRICS_PORT: u16 = 17873;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    #[serde(default = "country")]
    pub country: String,
}
fn country() -> String {
    "Свой сервер".into()
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        for (s, max) in [
            (&self.id, 64),
            (&self.name, 80),
            (&self.server, 253),
            (&self.username, 512),
            (&self.password, 512),
        ] {
            if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
                return Err("Некорректные поля профиля".into());
            }
        }
        if !self
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || self.port == 0
        {
            return Err("Некорректный идентификатор или порт".into());
        }
        if !self
            .server
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        {
            return Err("Укажите домен или IPv4 сервера".into());
        }
        Ok(())
    }
    pub fn public(&self, selected: &str) -> Value {
        json!({"id":self.id,"name":self.name,"server":self.server,"port":self.port,
               "country":self.country,"protocol":"Naive / HTTPS","selected":self.id==selected})
    }
}
fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}
pub fn parse_uri(input: &str) -> Result<Profile, String> {
    if input.len() > 8192 {
        return Err("Ссылка слишком длинная".into());
    }
    let u = Url::parse(input.trim()).map_err(|_| "Некорректная ссылка")?;
    if !["naive+https", "naive"].contains(&u.scheme())
        || u.username().is_empty()
        || u.password().is_none()
    {
        return Err("Нужна ссылка naive+https://логин:пароль@домен:443".into());
    }
    let server = u.host_str().ok_or("В ссылке нет сервера")?.to_string();
    let p = Profile {
        id: Uuid::new_v4().simple().to_string(),
        name: u
            .fragment()
            .map(decode)
            .filter(|s| !s.is_empty())
            .unwrap_or(server.clone()),
        server,
        port: u.port().unwrap_or(443),
        username: decode(u.username()),
        password: decode(u.password().unwrap()),
        country: country(),
    };
    p.validate()?;
    Ok(p)
}
pub fn parse_subscription(body: &str) -> Result<Vec<Profile>, String> {
    if body.len() > 1_048_576 {
        return Err("Подписка больше 1 МБ".into());
    }
    let mut profiles = vec![];
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        let items = v
            .get("outbounds")
            .and_then(Value::as_array)
            .ok_or("Нет списка серверов")?;
        for o in items.iter().filter(|o| o["type"] == "naive") {
            if o["quic"].as_bool() == Some(true) || o["tls"]["insecure"].as_bool() == Some(true) {
                return Err("Подписка требует неподдерживаемый или небезопасный режим TLS".into());
            }
            let server = o["server"]
                .as_str()
                .ok_or("Нет адреса сервера")?
                .to_string();
            if let Some(sni) = o["tls"]["server_name"].as_str() {
                if sni != server {
                    return Err("Домен TLS должен совпадать с адресом сервера".into());
                }
            }
            let port = o["server_port"].as_u64().unwrap_or(443);
            let p = Profile {
                id: Uuid::new_v4().simple().to_string(),
                name: o["tag"]
                    .as_str()
                    .unwrap_or("Свой сервер")
                    .chars()
                    .take(80)
                    .collect(),
                server,
                port: u16::try_from(port).map_err(|_| "Некорректный порт")?,
                username: o["username"].as_str().unwrap_or("").into(),
                password: o["password"].as_str().unwrap_or("").into(),
                country: country(),
            };
            p.validate()?;
            profiles.push(p);
        }
    } else {
        let content = if body.trim().starts_with("naive") {
            body.to_string()
        } else {
            let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = general_purpose::STANDARD
                .decode(&clean)
                .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(&clean))
                .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(&clean))
                .map_err(|_| "Непонятный формат подписки")?;
            String::from_utf8(bytes).map_err(|_| "Подписка должна быть UTF-8")?
        };
        for line in content
            .lines()
            .map(str::trim)
            .filter(|s| s.starts_with("naive"))
        {
            profiles.push(parse_uri(line)?);
        }
    }
    if profiles.is_empty() || profiles.len() > 32 {
        return Err("В подписке должно быть от 1 до 32 Naive HTTPS серверов".into());
    }
    Ok(profiles)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub mode: String,
    pub selected_profile: String,
    pub minimize_to_tray: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: "system".into(),
            selected_profile: String::new(),
            minimize_to_tray: true,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub subscription_url: String,
}

pub fn make_config(p: &Profile, mode: &str, secret: &str) -> Result<Value, String> {
    p.validate()?;
    if !["system", "proxy", "tun"].contains(&mode) {
        return Err("Неизвестный режим".into());
    }
    let mut ins =
        vec![json!({"type":"mixed","tag":"local","listen":"127.0.0.1","listen_port":PROXY_PORT})];
    if mode == "tun" {
        ins.push(json!({"type":"tun","tag":"tunnel","interface_name":"CorepassNext","address":["172.28.93.1/30","fdfe:dcba:9877::1/126"],"mtu":1400,"auto_route":true,"strict_route":true,"stack":"gvisor"}));
    }
    Ok(json!({"log":{"level":"warn","timestamp":true},
        "dns":{"servers":[{"type":"local","tag":"bootstrap"},{"type":"https","tag":"secure","server":"1.1.1.1","path":"/dns-query","detour":"vpn"}],"final":"secure","strategy":"prefer_ipv4"},
        "inbounds":ins,"outbounds":[{"type":"naive","tag":"vpn","server":p.server,"server_port":p.port,"username":p.username,"password":p.password,"udp_over_tcp":true,"quic":false,"tls":{"enabled":true,"server_name":p.server}},{"type":"direct","tag":"direct"}],
        "route":{"auto_detect_interface":true,"default_domain_resolver":"bootstrap","final":"vpn","rules":[{"action":"sniff"},{"protocol":"dns","action":"hijack-dns"},{"ip_is_private":true,"action":"route","outbound":"direct"}]},
        "experimental":{"clash_api":{"external_controller":format!("127.0.0.1:{METRICS_PORT}"),"secret":secret}}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uri_credentials_decode_and_public_excludes_secrets() {
        let p = parse_uri("naive+https://user:p%40ss@example.com:443#Test").unwrap();
        assert_eq!(p.password, "p@ss");
        assert!(p.public("").get("password").is_none());
        assert!(parse_uri("http://user:pass@example.com").is_err());
    }
    #[test]
    fn subscription_never_runs_arbitrary_settings() {
        let input = r#"{"inbounds":[{"type":"tun","auto_route":true}],"outbounds":[{"type":"naive","tag":"Test","server":"example.com","username":"alice","password":"secret"},{"type":"direct"}]}"#;
        let p = parse_subscription(input).unwrap();
        let cfg = make_config(&p[0], "proxy", "secret").unwrap();
        assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["inbounds"][0]["listen_port"], PROXY_PORT);
    }
    #[test]
    fn reject_invalid_port_control_and_insecure() {
        assert!(parse_uri("naive+https://user:pass@example.com:0").is_err());
        assert!(parse_uri("naive+https://user:pass@example.com#bad%0Aname").is_err());
        assert!(parse_subscription(r#"{"outbounds":[{"type":"naive","server":"example.com","username":"u","password":"p","tls":{"insecure":true}}]}"#).is_err());
    }
    #[test]
    fn ports_and_tun_do_not_collide_with_old_client() {
        let p = parse_uri("naive+https://user:pass@example.com").unwrap();
        let cfg = make_config(&p, "tun", "secret").unwrap();
        assert_ne!(API_PORT, 17861);
        assert_eq!(cfg["inbounds"][1]["interface_name"], "CorepassNext");
        assert_eq!(cfg["inbounds"][1]["stack"], "gvisor");
    }
}
