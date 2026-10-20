use std::{fs,path::Path,process::{Command,Child},time::{Duration,Instant}};
use serde_json::{json,Value};

pub fn quiet(command:&mut Command)->&mut Command {
    #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
    command
}
pub fn wait_child(child:&mut Child,seconds:u64)->Result<(),String> {
    let deadline=Instant::now()+Duration::from_secs(seconds);
    loop { match child.try_wait() {Ok(Some(status))=>return if status.success(){Ok(())}else{Err("Команда завершилась с ошибкой".into())},Ok(None)=>(),Err(_)=>return Err("Не удалось проверить процесс".into())};
        if Instant::now()>deadline {let _=child.kill();let _=child.wait();return Err("Истекло время ожидания процесса".into());} std::thread::sleep(Duration::from_millis(50));}
}
pub fn protect_directory(path:&Path)->Result<(),String> {
    fs::create_dir_all(path).map_err(|_|"Не удалось создать папку данных")?;
    #[cfg(windows)] {
        let output=quiet(Command::new("whoami.exe").args(["/user","/fo","csv","/nh"])).output().map_err(|_|"Не удалось определить пользователя Windows")?;
        let text=String::from_utf8_lossy(&output.stdout);
        let sid=regex::Regex::new(r"S-1-5-[0-9-]+").unwrap().find(&text).ok_or("Нет SID пользователя")?.as_str().to_string();
        let status=quiet(Command::new("icacls.exe").arg(path).args(["/inheritance:r","/grant:r",&format!("*{sid}:(OI)(CI)F"),"*S-1-5-18:(OI)(CI)F"])).output().map_err(|_|"Не удалось защитить папку данных")?;
        if !status.status.success(){return Err("Windows не разрешила защитить папку данных".into());}
    }
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(path,fs::Permissions::from_mode(0o700)).map_err(|_|"Не удалось защитить папку данных")?;}
    Ok(())
}
pub fn is_admin()->bool {
    #[cfg(windows)] {unsafe{windows_sys::Win32::UI::Shell::IsUserAnAdmin()!=0}}
    #[cfg(not(windows))] {false}
}

#[cfg(windows)]
pub struct Job(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)] unsafe impl Send for Job {}
#[cfg(not(windows))] pub struct Job;
impl Job {
    pub fn attach(child:&Child)->Result<Self,String> {
        #[cfg(windows)] {use windows_sys::Win32::{System::JobObjects::*,Foundation::CloseHandle};use std::os::windows::io::AsRawHandle;
            unsafe {let handle=CreateJobObjectW(std::ptr::null(),std::ptr::null());
                if handle.is_null(){return Err("Не удалось создать Job Object".into());}
                let mut info:JOBOBJECT_EXTENDED_LIMIT_INFORMATION=std::mem::zeroed();info.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(handle,JobObjectExtendedLimitInformation,&info as *const _ as *const _,std::mem::size_of_val(&info) as u32)==0 || AssignProcessToJobObject(handle,child.as_raw_handle())==0 {CloseHandle(handle);return Err("Не удалось безопасно привязать движок к приложению".into());}
                Ok(Self(handle))}
        }
        #[cfg(not(windows))] {let _=child;Ok(Self)}
    }
}
#[cfg(windows)] impl Drop for Job {fn drop(&mut self){unsafe{windows_sys::Win32::Foundation::CloseHandle(self.0);}}}

#[cfg(windows)] fn notify_proxy(){unsafe{windows_sys::Win32::Networking::WinInet::InternetSetOptionW(std::ptr::null_mut(),39,std::ptr::null(),0);windows_sys::Win32::Networking::WinInet::InternetSetOptionW(std::ptr::null_mut(),37,std::ptr::null(),0);}}
pub fn apply_proxy(data:&Path)->Result<(),String> {
    #[cfg(windows)] {
        use winreg::{RegKey,enums::*};
        let (key,_)=RegKey::predef(HKEY_CURRENT_USER).create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings").map_err(|_|"Не удалось открыть настройки прокси")?;
        let mut old=serde_json::Map::new();
        for name in ["ProxyEnable","ProxyServer","ProxyOverride"] {
            let v=key.get_raw_value(name).ok().map(|v|json!({"kind":v.vtype as u32,"bytes":v.bytes}));old.insert(name.into(),v.unwrap_or(Value::Null));
        }
        crate::atomic_json(&data.join("proxy-recovery.json"),&Value::Object(old))?;
        key.set_value("ProxyServer",&format!("127.0.0.1:{}",crate::model::PROXY_PORT)).map_err(|_|"Не удалось задать адрес прокси")?;
        key.set_value("ProxyOverride",&"<local>;localhost;127.*;10.*;192.168.*;172.16.*;172.17.*;172.18.*;172.19.*;172.2?.*;172.30.*;172.31.*").map_err(|_|"Не удалось задать исключения прокси")?;
        key.set_value("ProxyEnable",&1u32).map_err(|_|"Не удалось включить прокси")?;notify_proxy();Ok(())
    }
    #[cfg(not(windows))] {let _=data;Err("Системный прокси доступен в Windows".into())}
}
pub fn restore_proxy(data:&Path)->Result<(),String> {
    let path=data.join("proxy-recovery.json");if !path.exists(){return Ok(());}
    #[cfg(windows)] {
        use winreg::{RegKey,RegValue,enums::*};
        let key=RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",KEY_READ|KEY_WRITE).map_err(|_|"Не удалось восстановить прокси")?;
        let owns=key.get_value::<String,_>("ProxyServer").unwrap_or_default()==format!("127.0.0.1:{}",crate::model::PROXY_PORT);
        if owns {
            let old:Value=serde_json::from_slice(&fs::read(&path).map_err(|_|"Не читается снимок прокси")?).map_err(|_|"Повреждён снимок прокси")?;
            for name in ["ProxyEnable","ProxyServer","ProxyOverride"] {
                let v=&old[name];if v.is_null(){let _=key.delete_value(name);}else{
                    let kind=match v["kind"].as_u64(){Some(1)=>REG_SZ,Some(2)=>REG_EXPAND_SZ,Some(3)=>REG_BINARY,Some(4)=>REG_DWORD,Some(7)=>REG_MULTI_SZ,Some(11)=>REG_QWORD,_=>return Err("Неизвестный тип настройки прокси".into())};
                    let bytes:Vec<u8>=serde_json::from_value(v["bytes"].clone()).map_err(|_|"Повреждены настройки прокси")?;
                    key.set_raw_value(name,&RegValue{bytes,vtype:kind}).map_err(|_|"Ошибка восстановления прокси")?;
                }
            }notify_proxy();
        }
    }
    fs::remove_file(path).map_err(|_|"Не удалось завершить восстановление прокси")?;Ok(())
}
pub fn prepare_tun(config:&mut Value,p:&crate::model::Profile)->Result<(),String> {
    use std::net::ToSocketAddrs;
    let ip=(p.server.as_str(),p.port).to_socket_addrs().map_err(|_|"Не удалось определить IP VPN-сервера")?.find(|a|a.is_ipv4()).ok_or("У сервера нет доступного IPv4")?.ip().to_string();
    config["outbounds"][0]["server"]=json!(ip);
    config["inbounds"][1]["route_exclude_address"]=json!([format!("{ip}/32")]);
    #[cfg(windows)] {
        let script="$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $p=Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | Select-Object -ExpandProperty ifIndex; Get-NetIPConfiguration | Where-Object { $_.InterfaceIndex -in $p -and $_.IPv4DefaultGateway } | Sort-Object InterfaceIndex | Select-Object -First 1 -ExpandProperty InterfaceAlias";
        let mut child=quiet(Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",script]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null())).spawn().map_err(|_|"Не удалось найти сетевой адаптер")?;
        wait_child(&mut child,8)?;
        use std::io::Read;let mut bytes=vec![];if let Some(mut out)=child.stdout.take(){let _=out.read_to_end(&mut bytes);}
        // PowerShell writes UTF-8 explicitly through the console on modern Windows; request JSON uses an ASCII-safe alias fallback below.
        let alias=String::from_utf8_lossy(&bytes).trim().to_string();
        if alias.is_empty()||alias.contains('\u{fffd}') {return Err("Не найден физический адаптер. Используйте режим системного прокси".into());}
        config["outbounds"][0]["bind_interface"]=json!(alias);
    }
    Ok(())
}
pub fn wait_parent(pid:u32) {
    #[cfg(windows)] {use windows_sys::Win32::{System::Threading::*,Foundation::CloseHandle};unsafe{let h=OpenProcess(0x00100000,0,pid);if !h.is_null(){WaitForSingleObject(h,u32::MAX);CloseHandle(h);}}}
    #[cfg(not(windows))] {let _=pid;}
}
