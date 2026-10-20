use corepass_core::{
    atomic_json,
    model::{parse_subscription, Profile},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ETC: &str = "/etc/corepass";
const VAR: &str = "/var/run/corepass";
const ENGINE: &str = "/usr/lib/corepass/runtime/sing-box";
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    enabled: bool,
    subscription: String,
    devices: Vec<String>,
    lan_interface: String,
}
fn settings() -> Result<Config, String> {
    let p = Path::new(ETC).join("settings.json");
    if !p.exists() {
        return Ok(Config {
            lan_interface: "br-lan".into(),
            ..Config::default()
        });
    }
    serde_json::from_slice(&fs::read(p).map_err(|_| "Не читаются настройки")?)
        .map_err(|_| "Повреждены настройки".into())
}
fn persist(c: &Config) -> Result<(), String> {
    atomic_json(
        &Path::new(ETC).join("settings.json"),
        &serde_json::to_value(c).unwrap(),
    )
}
fn profile() -> Result<Profile, String> {
    let p: Profile = serde_json::from_slice(
        &fs::read(Path::new(ETC).join("profile.json"))
            .map_err(|_| "Добавьте HTTPS-подписку в панели")?,
    )
    .map_err(|_| "Повреждён профиль")?;
    p.validate()?;
    Ok(p)
}
fn valid_mac(mac: &str) -> bool {
    let parts: Vec<_> = mac.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|c| c.is_ascii_hexdigit()))
        && u8::from_str_radix(parts[0], 16)
            .map(|v| v & 1 == 0)
            .unwrap_or(false)
        && mac != "00:00:00:00:00:00"
}
fn validate(c: &Config) -> Result<(), String> {
    if c.devices.len() > 32 || c.devices.iter().any(|s| !valid_mac(s)) {
        return Err("Укажите до 32 MAC-адресов устройств, например AA:BB:CC:DD:EE:FF".into());
    }
    if c.lan_interface.is_empty()
        || c.lan_interface.len() > 15
        || !c
            .lan_interface
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return Err("Некорректный LAN-интерфейс".into());
    }
    if !c.subscription.is_empty() {
        let u = url::Url::parse(&c.subscription).map_err(|_| "Некорректная ссылка подписки")?;
        if u.scheme() != "https"
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || c.subscription.len() > 4096
        {
            return Err("Нужна HTTPS-подписка без логина и пароля в URL".into());
        }
    }
    Ok(())
}
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Не удалось создать HTTPS-клиент".into())
}
fn fetch_profile(link: &str) -> Result<Profile, String> {
    let response = client()?
        .get(link)
        .send()
        .map_err(|_| "Не удалось загрузить подписку. Проверьте DNS и доступ к домену сервера")?;
    if !response.status().is_success() {
        return Err(format!(
            "Подписка вернула HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut body = String::new();
    response
        .take(1_048_577)
        .read_to_string(&mut body)
        .map_err(|_| "Подписка должна быть UTF-8")?;
    let profiles = parse_subscription(&body)?;
    if profiles.len() != 1 {
        return Err(
            "Для роутера используйте персональную подписку с одним Naive HTTPS сервером".into(),
        );
    }
    Ok(profiles.into_iter().next().unwrap())
}
fn render(c: &Config, p: &Profile) -> Result<Value, String> {
    validate(c)?;
    p.validate()?;
    if c.devices.is_empty() {
        return Err(
            "Выберите хотя бы одно тестовое устройство. Пустой список не означает всю сеть".into(),
        );
    }
    Ok(json!({"log":{"level":"warn","timestamp":true},
      "dns":{"servers":[{"type":"local","tag":"bootstrap"},{"type":"https","tag":"secure","server":"1.1.1.1","path":"/dns-query","detour":"vpn"}],"final":"secure","strategy":"prefer_ipv4"},
      "inbounds":[{"type":"mixed","tag":"test-proxy","listen":"127.0.0.1","listen_port":17882},
        {"type":"tun","tag":"lan-tun","interface_name":"corepass0","address":["172.28.94.1/30","fdfe:dcba:9878::1/126"],"mtu":1400,"auto_route":true,"auto_redirect":true,"strict_route":true,"stack":"system",
         "include_interface":[c.lan_interface],"include_mac_address":c.devices,"iproute2_table_index":4343,"iproute2_rule_index":21430,"auto_redirect_input_mark":"0x43101","auto_redirect_output_mark":"0x43102","auto_redirect_reset_mark":"0x43103","auto_redirect_nfqueue":143}],
      "outbounds":[{"type":"naive","tag":"vpn","server":p.server,"server_port":p.port,"username":p.username,"password":p.password,"stream_receive_window":"4 MB","udp_over_tcp":true,"quic":false,"tls":{"enabled":true,"server_name":p.server}},{"type":"direct","tag":"direct"}],
      "route":{"auto_detect_interface":true,"default_domain_resolver":"bootstrap","final":"vpn","rules":[{"action":"sniff"},{"protocol":"dns","action":"hijack-dns"},{"ip_is_private":true,"action":"route","outbound":"direct"}]}}))
}
fn command(program: &str, args: &[&str], timeout: u64) -> Result<std::process::Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| format!("Не удалось запустить {program}"))?;
    // Drain stdout concurrently to avoid a pipe-capacity deadlock with logread/nft.
    let out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut data = vec![];
        let _ = out.take(131072).read_to_end(&mut data);
        data
    });
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|_| "Ошибка контроля процесса")?
        {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Команда не ответила вовремя".into());
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let stdout = reader.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr: vec![],
    })
}
fn conflicts() -> Vec<String> {
    let mut found = vec![];
    if let Ok(out) = command("nft", &["-j", "list", "tables"], 3) {
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        for name in ["steer", "splify", "netshift", "mihomo", "passwall"] {
            if text.contains(name) {
                found.push(name.into());
            }
        }
    }
    if let Ok(entries) = fs::read_dir("/proc") {
        for item in entries.flatten() {
            if !item
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|c| c.is_ascii_digit())
            {
                continue;
            }
            let name = fs::read_to_string(item.path().join("comm")).unwrap_or_default();
            let name = name.trim();
            if ["steer", "mihomo", "netshift", "xray"].contains(&name) {
                found.push(name.into());
            }
            if name == "sing-box" {
                if let Ok(path) = fs::read_link(item.path().join("exe")) {
                    if path != PathBuf::from(ENGINE) {
                        found.push("другой sing-box".into());
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}
fn service_pid() -> Option<u64> {
    let out = command(
        "ubus",
        &["call", "service", "list", r#"{"name":"corepass"}"#],
        3,
    )
    .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    v["corepass"]["instances"]
        .as_object()?
        .values()
        .find(|x| x["running"] == true)
        .and_then(|x| x["pid"].as_u64())
}
fn prepare() -> Result<Value, String> {
    let c = settings()?;
    if !c.enabled {
        return Err("Corepass выключен в настройках".into());
    }
    let p = profile()?;
    let mut config = render(&c, &p)?;
    let other = conflicts();
    if !other.is_empty() {
        return Err(format!("Работает другая маршрутизация: {}. Для первого теста остановите её через Zapret Manager; настройки сохранятся",other.join(", ")));
    }
    if !Path::new("/dev/net/tun").exists() {
        return Err("Нет /dev/net/tun. Установите kmod-tun".into());
    }
    corepass_core::platform::protect_directory(Path::new(VAR))?;
    let wan = command("ubus", &["call", "network.interface.wan", "status"], 4)?;
    let wan: Value =
        serde_json::from_slice(&wan.stdout).map_err(|_| "Не удалось определить WAN-интерфейс")?;
    let iface = wan["l3_device"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("WAN не подключён или имеет нестандартное имя. Проверьте интерфейс wan в OpenWrt")?;
    config["outbounds"][0]["bind_interface"] = json!(iface);
    let path = Path::new(VAR).join("config.json");
    atomic_json(&path, &config)?;
    let out = command(ENGINE, &["check", "-c", path.to_str().unwrap()], 12)?;
    if !out.status.success() {
        return Err("Движок отклонил конфигурацию; подключение не включено".into());
    }
    Ok(json!({"prepared":true}))
}
fn status() -> Value {
    let c = settings().unwrap_or_default();
    let pid = service_pid();
    let check = fs::read(Path::new(VAR).join("lastcheck.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let check = check.filter(|v| {
        v["pid"].as_u64() == pid
            && pid.is_some()
            && now.saturating_sub(v["time"].as_u64().unwrap_or(0)) < 120
    });
    let memory = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let available = memory
        .lines()
        .find(|s| s.starts_with("MemAvailable:"))
        .and_then(|s| s.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    json!({"version":env!("CARGO_PKG_VERSION"),"running":pid.is_some(),"pid":pid,"enabled":c.enabled,"configured":Path::new(ETC).join("profile.json").exists(),"devices":c.devices,"lan_interface":c.lan_interface,"other_routing":conflicts(),"check":check,"available_memory_kib":available})
}
fn save(body: &Value) -> Result<Value, String> {
    if service_pid().is_some() {
        return Err("Остановите Corepass перед изменением настроек".into());
    }
    let mut c = settings()?;
    if let Some(v) = body.get("devices") {
        c.devices = serde_json::from_value(v.clone()).map_err(|_| "Укажите список MAC-адресов")?;
        for mac in &mut c.devices {
            *mac = mac.to_uppercase();
        }
        c.devices.sort();
        c.devices.dedup();
    }
    if let Some(v) = body.get("lan_interface") {
        c.lan_interface = v.as_str().ok_or("Нужен LAN-интерфейс")?.into();
    }
    let link = body["subscription"].as_str().unwrap_or("").trim();
    let new_profile = if !link.is_empty() {
        c.subscription = link.to_string();
        validate(&c)?;
        Some(fetch_profile(link)?)
    } else {
        None
    };
    validate(&c)?;
    if let Some(p) = new_profile {
        atomic_json(
            &Path::new(ETC).join("profile.json"),
            &serde_json::to_value(p).unwrap(),
        )?;
    }
    persist(&c)?;
    Ok(json!({"saved":true,"message":"Настройки сохранены. Подключение не включалось."}))
}
fn change_running(enabled: bool) -> Result<Value, String> {
    let mut c = settings()?;
    if enabled {
        let p = profile()?;
        render(&c, &p)?;
        let other = conflicts();
        if !other.is_empty() {
            return Err(format!("Сначала остановите другую маршрутизацию в менеджере: {}. Corepass сам её не выключает.",other.join(", ")));
        }
    }
    c.enabled = enabled;
    persist(&c)?;
    if enabled {
        if let Err(e) = prepare() {
            c.enabled = false;
            let _ = persist(&c);
            return Err(e);
        }
        let out = command("/etc/init.d/corepass", &["start"], 12)?;
        if !out.status.success() {
            c.enabled = false;
            let _ = persist(&c);
            return Err("Служба не запустилась. Посмотрите журнал.".into());
        }
        let _ = command("/etc/init.d/corepass", &["enable"], 4)?;
    } else {
        let _ = command("/etc/init.d/corepass", &["stop"], 12)?;
        let _ = command("/etc/init.d/corepass", &["disable"], 4)?;
        let _ = fs::remove_file(Path::new(VAR).join("lastcheck.json"));
    }
    Ok(
        json!({"message":if enabled{"Запуск запрошен. Проверьте соединение кнопкой «Проверить выход»."}else{"Corepass остановлен. Остальные службы не менялись."}}),
    )
}
fn check() -> Result<Value, String> {
    let pid = service_pid().ok_or("Corepass не запущен")?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .proxy(
            reqwest::Proxy::all("http://127.0.0.1:17882")
                .map_err(|_| "Ошибка локального прокси")?,
        )
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Ошибка HTTPS-клиента")?;
    let started = Instant::now();
    let response = client
        .get("https://api.ipify.org")
        .send()
        .map_err(|_| "HTTPS через VPN не отвечает")?;
    if !response.status().is_success() {
        return Err("Проверка выхода вернула ошибку HTTP".into());
    }
    let mut text = String::new();
    response
        .take(128)
        .read_to_string(&mut text)
        .map_err(|_| "Не читается ответ проверки")?;
    let ip: std::net::IpAddr = text
        .trim()
        .parse()
        .map_err(|_| "Не получен корректный IP")?;
    let v = json!({"pid":pid,"exit_ip":ip.to_string(),"latency_ms":started.elapsed().as_millis() as u64,"time":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),"message":"HTTPS через локальный VPN-прокси проверен. Маршрут LAN-устройства проверяйте на самом устройстве."});
    atomic_json(&Path::new(VAR).join("lastcheck.json"), &v)?;
    Ok(v)
}
fn logs() -> Result<Value, String> {
    let output = command("logread", &["-l", "80", "-e", "corepass"], 4)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let log = corepass_core::logging::Journal::new(Path::new(VAR));
    if let Ok(c) = settings() {
        log.register([c.subscription]);
    }
    if let Ok(p) = profile() {
        log.register([p.username, p.password]);
    }
    let safe = log.redact(
        &text
            .lines()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    Ok(json!({"log":safe}))
}
fn dispatch(method: &str, body: &Value) -> Result<Value, String> {
    if !body.is_object() {
        return Err("Нужен JSON-объект".into());
    }
    match method {
        "status" => Ok(status()),
        "get_config" => {
            let c = settings()?;
            Ok(
                json!({"devices":c.devices,"lan_interface":c.lan_interface,"configured":!c.subscription.is_empty()}),
            )
        }
        "save" => save(body),
        "start" => change_running(true),
        "stop" => change_running(false),
        "check" => check(),
        "logs" => logs(),
        "prepare" => prepare(),
        "install-disabled" => {
            let mut c = settings()?;
            c.enabled = false;
            persist(&c)?;
            Ok(json!({"installed_disabled":true}))
        }
        _ => Err("Неизвестная команда".into()),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("list") {
        println!(
            "{}",
            json!({"status":{},"get_config":{},"save":{"subscription":"String","devices":"Array","lan_interface":"String"},"start":{},"stop":{},"check":{},"logs":{}})
        );
        return;
    }
    let method = if args.get(1).map(String::as_str) == Some("call") {
        args.get(2).map(String::as_str).unwrap_or("")
    } else {
        args.get(1).map(String::as_str).unwrap_or("")
    };
    let mut input = String::new();
    if matches!(args.get(1).map(String::as_str), Some("call" | "render")) {
        let _ = std::io::stdin().take(16385).read_to_string(&mut input);
    }
    let body = if input.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&input).unwrap_or(Value::Null)
    };
    let result = if input.len() > 16384 {
        Err("Запрос слишком большой".into())
    } else if args.get(1).map(String::as_str) == Some("render") {
        let config = serde_json::from_value::<Config>(body["config"].clone())
            .map_err(|_| "Неверные настройки".to_string());
        let profile = serde_json::from_value::<Profile>(body["profile"].clone())
            .map_err(|_| "Неверный профиль".to_string());
        config.and_then(|c| profile.and_then(|p| render(&c, &p)))
    } else {
        dispatch(method, &body)
    };
    match result {
        Ok(v) => println!("{v}"),
        Err(message) => {
            println!("{}", json!({"error":message}));
            if method == "prepare" || method == "install-disabled" {
                std::process::exit(1);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p() -> Profile {
        corepass_core::model::parse_uri("naive+https://test:secret@example.com").unwrap()
    }
    #[test]
    fn reject_empty_devices_and_invalid_macs() {
        let mut c = Config {
            lan_interface: "br-lan".into(),
            ..Config::default()
        };
        assert!(render(&c, &p()).is_err());
        c.devices = vec!["AA:BB:CC:DD:EE:FF".into()];
        assert!(render(&c, &p()).is_ok());
        c.devices = vec!["FF:FF:FF:FF:FF:FF".into()];
        assert!(render(&c, &p()).is_err());
    }
    #[test]
    fn router_limits_capture_and_memory() {
        let c = Config {
            devices: vec!["AA:BB:CC:DD:EE:FF".into()],
            lan_interface: "br-lan".into(),
            ..Config::default()
        };
        let v = render(&c, &p()).unwrap();
        assert_eq!(v["inbounds"][1]["include_mac_address"], json!(c.devices));
        assert_eq!(v["outbounds"][0]["stream_receive_window"], "4 MB");
        assert_eq!(v["inbounds"][1]["auto_redirect_nfqueue"], 143);
    }
    #[test]
    fn reject_shell_syntax_and_insecure_links() {
        let c = Config {
            lan_interface: "br-lan;reboot".into(),
            ..Config::default()
        };
        assert!(validate(&c).is_err());
        let c = Config {
            lan_interface: "br-lan".into(),
            subscription: "http://example.com".into(),
            ..Config::default()
        };
        assert!(validate(&c).is_err());
    }
    #[test]
    fn rpc_has_no_shell_execution() {
        assert!(dispatch("exec", &json!({"command":"reboot"})).is_err());
    }
}
