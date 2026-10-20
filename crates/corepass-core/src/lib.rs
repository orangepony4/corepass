pub mod logging;
pub mod model;
pub mod platform;

use model::*;
use logging::Journal;
use serde_json::{json,Value};
use std::{fs,io::{BufRead,BufReader,Read},net::{SocketAddr,TcpStream,TcpListener},path::{Path,PathBuf},process::{Child,Command,Stdio},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};

pub fn atomic_json(path:&Path,value:&Value)->Result<(),String> {
    let tmp=path.with_extension("tmp");
    fs::write(&tmp,serde_json::to_vec_pretty(value).map_err(|_|"Ошибка сохранения JSON")?).map_err(|_|"Ошибка записи настроек")?;
    // MoveFileEx replacement is atomic on Windows; the file is on the same volume.
    #[cfg(windows)] {use std::os::windows::ffi::OsStrExt;let a:Vec<u16>=tmp.as_os_str().encode_wide().chain(Some(0)).collect();let b:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {unsafe extern "system" {fn MoveFileExW(a:*const u16,b:*const u16,flags:u32)->i32;} if MoveFileExW(a.as_ptr(),b.as_ptr(),9)==0{return Err("Ошибка атомарного сохранения настроек".into());}}
    }
    #[cfg(not(windows))] fs::rename(tmp,path).map_err(|_|"Ошибка атомарного сохранения настроек")?;
    Ok(())
}
pub struct ApiError {pub status:u16,pub code:String,pub message:String}
impl From<String> for ApiError {fn from(message:String)->Self{Self{status:400,code:"CP-REQUEST".into(),message}}}
impl From<&str> for ApiError {fn from(message:&str)->Self{message.to_string().into()}}
pub type ApiResult=Result<Value,ApiError>;
fn error(status:u16,code:&str,message:&str)->ApiError{ApiError{status,code:code.into(),message:message.into()}}

struct Engine {child:Child,_job:platform::Job}
struct State {
    profiles:ProfileStore,settings:Settings,engine:Option<Engine>,started:Option<Instant>,status:String,message:String,
    exit_ip:Option<String>,latency:Option<u64>,last_error:Option<Value>,checks:Vec<Value>,operations:Vec<Value>,
    upload:u64,download:u64,upload_rate:u64,download_rate:u64,history:Vec<Value>,
}
pub struct App {
    pub data:PathBuf,pub engine_path:PathBuf,pub log:Arc<Journal>,pub token:String,
    state:Mutex<State>,busy:AtomicBool,cancel:AtomicBool,pub closing:AtomicBool,
    pub show_requested:AtomicBool,
}

fn read_json(path:&Path)->Result<Value,String>{serde_json::from_slice(&fs::read(path).map_err(|_|"Не читается файл настроек")?).map_err(|_|"Повреждён файл настроек".into())}
pub fn legacy_present()->bool {TcpStream::connect_timeout(&SocketAddr::from(([127,0,0,1],17861)),Duration::from_millis(150)).is_ok()}
fn http_client(proxy:bool)->Result<reqwest::blocking::Client,String> {
    let builder=reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(4)).timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).no_proxy();
    let builder=if proxy{builder.proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{PROXY_PORT}")).map_err(|_|"Ошибка локального прокси")?)}else{builder};
    builder.build().map_err(|_|"Ошибка HTTPS-клиента".into())
}
impl App {
    pub fn new(data:PathBuf,engine_path:PathBuf)->Result<Arc<Self>,String>{
        platform::protect_directory(&data)?;
        let log=Arc::new(Journal::new(&data.join("logs")));
        let token_path=data.join("api-token");
        let token=if token_path.exists(){fs::read_to_string(&token_path).map_err(|_|"Не читается токен API")?.trim().to_string()}else{
            let value=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());fs::write(&token_path,&value).map_err(|_|"Не сохраняется токен API")?;value};
        if token.len()!=64{return Err("Повреждён токен API".into());}
        let profiles:ProfileStore=if data.join("profiles.json").exists(){serde_json::from_value(read_json(&data.join("profiles.json"))?).map_err(|_|"Повреждён список профилей")?}else{ProfileStore::default()};
        for p in &profiles.profiles{p.validate()?;}
        let settings:Settings=if data.join("settings.json").exists(){serde_json::from_value(read_json(&data.join("settings.json"))?).map_err(|_|"Повреждены настройки")?}else{Settings::default()};
        log.register([token.clone(),profiles.subscription_url.clone()]);for p in &profiles.profiles{log.register([p.username.clone(),p.password.clone()]);}
        platform::restore_proxy(&data)?;
        let app=Arc::new(Self{data,engine_path,log,state:Mutex::new(State{profiles,settings,engine:None,started:None,status:"disconnected".into(),message:"Готов к подключению".into(),exit_ip:None,latency:None,last_error:None,checks:vec![],operations:vec![],upload:0,download:0,upload_rate:0,download_rate:0,history:vec![]}),token,busy:AtomicBool::new(false),cancel:AtomicBool::new(false),closing:AtomicBool::new(false),show_requested:AtomicBool::new(false)});
        app.log.event("info","APP_READY","Corepass 0.3 · Rust backend ready");
        let a=app.clone();std::thread::spawn(move||{while !a.closing.load(Ordering::SeqCst){std::thread::sleep(Duration::from_secs(2));a.monitor();}});
        Ok(app)
    }
    pub fn status(&self)->Value {
        let legacy=legacy_present();let s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        json!({"version":VERSION,"state":s.status,"message":s.message,"busy":self.busy.load(Ordering::SeqCst),"mode":s.settings.mode,"profile_id":s.settings.selected_profile,
          "engine_running":s.engine.is_some(),"engine_available":self.engine_path.is_file(),"legacy_active":legacy,"is_admin":platform::is_admin(),
          "uptime_seconds":s.started.map(|t|t.elapsed().as_secs()).unwrap_or(0),"exit_ip":s.exit_ip,"latency_ms":s.latency,"last_error":s.last_error,
          "download_bytes":s.download,"upload_bytes":s.upload,"download_rate":s.download_rate,"upload_rate":s.upload_rate,"history":s.history,"diagnostics":s.checks,
          "api_url":format!("http://127.0.0.1:{API_PORT}"),"token_path":self.data.join("api-token").display().to_string(),"data_dir":self.data.display().to_string()})
    }
    fn public_profiles(&self)->Value{let s=self.state.lock().unwrap_or_else(|e|e.into_inner());json!({"profiles":s.profiles.profiles.iter().map(|p|p.public(&s.settings.selected_profile)).collect::<Vec<_>>()})}
    fn ensure_idle(&self)->Result<(),ApiError>{let s=self.state.lock().unwrap_or_else(|e|e.into_inner());if self.busy.load(Ordering::SeqCst)||s.engine.is_some(){Err(error(409,"CP-BUSY","Сначала завершите операцию и отключите новую версию"))}else{Ok(())}}
    fn persist(&self,s:&State)->Result<(),String>{atomic_json(&self.data.join("profiles.json"),&serde_json::to_value(&s.profiles).unwrap())?;atomic_json(&self.data.join("settings.json"),&serde_json::to_value(&s.settings).unwrap())}
    fn selected(&self)->Result<Profile,String>{let s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.profiles.profiles.iter().find(|p|p.id==s.settings.selected_profile).cloned().ok_or("Выберите профиль сервера".into())}
    fn check_cancel(&self)->Result<(),String>{if self.cancel.load(Ordering::SeqCst)||self.closing.load(Ordering::SeqCst){Err("Операция отменена".into())}else{Ok(())}}
    pub fn request(self:&Arc<Self>,method:&str,path:&str,body:Value)->ApiResult {
        if method=="GET" {return match path {
            "/health"=>Ok(json!({"app":"Corepass Next","version":VERSION,"backend":"Rust"})),
            "/v1/status"=>Ok(self.status()),"/v1/profiles"=>Ok(self.public_profiles()),"/v1/logs"=>Ok(json!({"events":self.log.read()})),
            "/v1/settings"=>Ok(serde_json::to_value(&self.state.lock().unwrap_or_else(|e|e.into_inner()).settings).unwrap()),
            "/v1/openapi.json"=>Ok(api_schema()),
            _ if path.starts_with("/v1/operations/")=>{let id=path.trim_start_matches("/v1/operations/");self.state.lock().unwrap_or_else(|e|e.into_inner()).operations.iter().find(|v|v["id"]==id).cloned().ok_or(error(404,"CP-NOT-FOUND","Операция не найдена"))},
            _=>Err(error(404,"CP-NOT-FOUND","Маршрут не найден"))};}
        if method!="POST" {return Err(error(405,"CP-METHOD","Метод не поддерживается"));}
        let obj=body.as_object().ok_or("Нужен JSON-объект")?;
        let allowed:&[&str]=match path {"/v1/profiles/import"=>&["uri"],"/v1/profiles/subscribe"=>&["url"],"/v1/profiles/select"=>&["profile_id"],"/v1/settings"=>&["mode","minimize_to_tray"],"/v1/connect"=>&["mode"],_=>&[]};
        if obj.keys().any(|key|!allowed.contains(&key.as_str())){return Err("Неизвестные параметры команды".into());}
        match path {
            "/v1/connect"|"/v1/disconnect"|"/v1/diagnostics"|"/v1/profiles/refresh"|"/v1/profiles/subscribe"=>self.submit(path,body),
            "/v1/cancel"=>{self.cancel.store(true,Ordering::SeqCst);Ok(json!({"cancelling":true}))},
            "/v1/profiles/import"=>{self.ensure_idle()?;let p=parse_uri(body["uri"].as_str().ok_or("Нужна ссылка uri")?)?;self.add_profiles(vec![p],None)?;Ok(self.public_profiles())},
            "/v1/profiles/migrate"=>{self.ensure_idle()?;let source=self.data.parent().ok_or("Нет папки пользователя")?.join("Corepass/profiles.json");let old:ProfileStore=serde_json::from_value(read_json(&source)?).map_err(|_|"Не удалось прочитать старые профили")?;
                if old.profiles.is_empty()||old.profiles.len()>32{return Err("В старом клиенте нет подходящих профилей".into());}for p in &old.profiles{p.validate()?;}self.add_profiles(old.profiles,Some(old.subscription_url))?;self.log.event("info","PROFILES_MIGRATED","Профили скопированы из Corepass 0.2; исходный файл не менялся");Ok(self.public_profiles())},
            "/v1/profiles/select"=>{self.ensure_idle()?;let id=body["profile_id"].as_str().ok_or("Нужен profile_id")?;let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());if !s.profiles.profiles.iter().any(|p|p.id==id){return Err("Профиль не найден".into());}s.settings.selected_profile=id.into();self.persist(&s)?;drop(s);Ok(self.public_profiles())},
            "/v1/settings"=>{self.ensure_idle()?;let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());if let Some(mode)=body.get("mode"){let m=mode.as_str().ok_or("Неверный режим")?;if !["system","tun","proxy"].contains(&m){return Err("Неверный режим".into());}s.settings.mode=m.into();}
                if let Some(v)=body.get("minimize_to_tray"){s.settings.minimize_to_tray=v.as_bool().ok_or("Нужно логическое значение")?;}self.persist(&s)?;Ok(serde_json::to_value(&s.settings).unwrap())},
            "/v1/support/export"=>{let report=json!({"version":VERSION,"created_at":chrono::Local::now().to_rfc3339(),"status":self.status(),"logs":self.log.read()});let path=self.data.join(format!("report-{}.json",chrono::Local::now().format("%Y%m%d-%H%M%S")));
                let safe=self.log.redact(&serde_json::to_string_pretty(&report).unwrap());fs::write(&path,safe).map_err(|_|"Не удалось сохранить отчёт")?;Ok(json!({"path":path.display().to_string()}))},
            "/v1/window/show"=>{self.show_requested.store(true,Ordering::SeqCst);Ok(json!({"shown":true}))},
            _=>Err(error(404,"CP-NOT-FOUND","Маршрут не найден"))
        }
    }
    fn add_profiles(&self,profiles:Vec<Profile>,subscription:Option<String>)->Result<(),String>{
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());
        for mut p in profiles {p.validate()?;self.log.register([p.username.clone(),p.password.clone()]);
            if let Some(existing)=s.profiles.profiles.iter_mut().find(|a|a.server==p.server&&a.port==p.port&&a.username==p.username){p.id=existing.id.clone();*existing=p;}else{if s.profiles.profiles.len()>=32{return Err("Максимум 32 профиля".into());}s.profiles.profiles.push(p);}}
        if !s.profiles.profiles.iter().any(|p|p.id==s.settings.selected_profile){s.settings.selected_profile=s.profiles.profiles[0].id.clone();}
        if let Some(url)=subscription{self.log.register([url.clone()]);s.profiles.subscription_url=url;}
        self.persist(&s)
    }
    fn submit(self:&Arc<Self>,path:&str,body:Value)->ApiResult {
        if path=="/v1/connect" && legacy_present(){return Err(error(409,"CP-LEGACY-ACTIVE","Старый Corepass работает. Новое подключение заблокировано, чтобы сохранить текущую сеть"));}
        if self.busy.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){return Err(error(409,"CP-BUSY","Выполняется другая операция"));}
        let id=uuid::Uuid::new_v4().simple().to_string();self.cancel.store(false,Ordering::SeqCst);
        {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());if s.operations.len()>=20{s.operations.remove(0);}s.operations.push(json!({"id":id,"action":path,"state":"running"}));}
        let a=self.clone();let action=path.to_string();let op_id=id.clone();
        std::thread::spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||match action.as_str(){
                "/v1/connect"=>a.connect(&body),"/v1/disconnect"=>{a.stop_engine();Ok(json!({"disconnected":true}))},
                "/v1/diagnostics"=>a.diagnose(),"/v1/profiles/refresh"=>a.subscribe(None),"/v1/profiles/subscribe"=>a.subscribe(body["url"].as_str()),_=>Err("Неизвестная операция".into())
            })).unwrap_or_else(|_|Err("Внутренняя ошибка. Подробности сохранены в журнале".into()));
            if result.is_err()&&action=="/v1/connect"{a.stop_engine();}
            let mut s=a.state.lock().unwrap_or_else(|e|e.into_inner());
            match result {Ok(v)=>{if let Some(op)=s.operations.iter_mut().find(|o|o["id"]==op_id){op["state"]=json!("succeeded");op["result"]=v;}a.log.event("info","OP_SUCCEEDED",&action);},Err(message)=>{
                let msg=a.log.redact(&message);let code=format!("CP-{}",&op_id[..8]);s.last_error=Some(json!({"code":code,"message":msg}));if action=="/v1/connect"{s.status="error".into();s.message=msg.clone();}
                if let Some(op)=s.operations.iter_mut().find(|o|o["id"]==op_id){op["state"]=json!(if a.cancel.load(Ordering::SeqCst){"cancelled"}else{"failed"});op["error"]=json!(msg);op["error_code"]=json!(code);}
                a.log.event("error",&code,&msg);
            }}a.busy.store(false,Ordering::SeqCst);
        });
        Ok(json!({"operation_id":id,"state":"accepted"}))
    }
    fn connect(self:&Arc<Self>,body:&Value)->Result<Value,String>{
        self.check_cancel()?;if legacy_present(){return Err("Старый Corepass работает — подключение новой версии запрещено".into());}
        {let s=self.state.lock().unwrap_or_else(|e|e.into_inner());if s.engine.is_some(){return Err("Новая версия уже подключена".into());}}
        let p=self.selected()?;let mode=body["mode"].as_str().map(str::to_owned).unwrap_or_else(||self.state.lock().unwrap_or_else(|e|e.into_inner()).settings.mode.clone());
        if mode=="tun"&&!platform::is_admin(){return Err("Для режима «Весь компьютер» запустите новую версию от администратора".into());}
        for port in [PROXY_PORT,METRICS_PORT] {let socket=TcpListener::bind(("127.0.0.1",port)).map_err(|_|format!("Локальный порт {port} занят"))?;drop(socket);}
        if !self.engine_path.is_file(){return Err("Не найден движок sing-box. Распакуйте полный архив приложения".into());}
        let metrics_secret=format!("{}",uuid::Uuid::new_v4().simple());self.log.register([metrics_secret.clone()]);
        let mut config=make_config(&p,&mode,&metrics_secret)?;if mode=="tun"{platform::prepare_tun(&mut config,&p)?;}
        atomic_json(&self.data.join("runtime.json"),&config)?;
        let mut checker=platform::quiet(Command::new(&self.engine_path).args(["check","-c"]).arg(self.data.join("runtime.json")).stdout(Stdio::null()).stderr(Stdio::null())).spawn().map_err(|_|"Не удалось запустить проверку sing-box")?;
        platform::wait_child(&mut checker,12).map_err(|_|"Движок отклонил конфигурацию")?;self.check_cancel()?;
        {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.status="connecting".into();s.message="Проверяем защищённый маршрут…".into();s.last_error=None;s.settings.mode=mode.clone();self.persist(&s)?;}
        let mut child=platform::quiet(Command::new(&self.engine_path).args(["run","-c"]).arg(self.data.join("runtime.json")).current_dir(self.engine_path.parent().unwrap()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())).spawn().map_err(|_|"Не удалось запустить sing-box")?;
        let job=match platform::Job::attach(&child){Ok(j)=>j,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e);}};
        if let Some(out)=child.stdout.take(){let log=self.log.clone();std::thread::spawn(move||{for line in BufReader::new(out).lines().map_while(Result::ok){log.event("info","ENGINE",&line);}});}
        if let Some(out)=child.stderr.take(){let log=self.log.clone();std::thread::spawn(move||{for line in BufReader::new(out).lines().map_while(Result::ok){log.event("warn","ENGINE",&line);}});}
        self.log.event("info","ENGINE_STARTED",&format!("Managed PID {}",child.id()));
        self.state.lock().unwrap_or_else(|e|e.into_inner()).engine=Some(Engine{child,_job:job});
        for attempt in 0..40 {self.check_cancel()?;if TcpStream::connect_timeout(&SocketAddr::from(([127,0,0,1],PROXY_PORT)),Duration::from_millis(100)).is_ok(){break;}if attempt==39{return Err("Движок не открыл локальный прокси".into());}std::thread::sleep(Duration::from_millis(100));}
        let result=self.probe("https://api.ipify.org",true)?;self.check_cancel()?;
        if mode=="system"{platform::apply_proxy(&self.data)?;}
        {let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.status="connected".into();s.message="Соединение проверено".into();s.exit_ip=result["exit_ip"].as_str().map(str::to_owned);s.latency=result["latency_ms"].as_u64();s.started=Some(Instant::now());s.download=0;s.upload=0;s.history.clear();}
        let a=self.clone();std::thread::spawn(move||a.metrics(metrics_secret));
        Ok(result)
    }
    fn probe(&self,url:&str,read_ip:bool)->Result<Value,String>{
        let t=Instant::now();let response=http_client(true)?.get(url).send().map_err(|_|"HTTPS-проверка через VPN не прошла")?;
        if !response.status().is_success(){return Err(format!("HTTPS-проверка вернула HTTP {}",response.status().as_u16()));}
        let mut v=json!({"host":UrlHost::of(url),"status":"ok","http_status":response.status().as_u16(),"latency_ms":t.elapsed().as_millis() as u64});
        if read_ip {let mut text=String::new();response.take(128).read_to_string(&mut text).map_err(|_|"Нет ответа проверки IP")?;let ip:std::net::IpAddr=text.trim().parse().map_err(|_|"Сервис проверки не вернул IP-адрес")?;v["exit_ip"]=json!(ip.to_string());}Ok(v)
    }
    fn diagnose(&self)->Result<Value,String>{
        if self.state.lock().unwrap_or_else(|e|e.into_inner()).engine.is_none(){return Err("Подключите новую версию перед диагностикой".into());}
        let mut checks=vec![];for (url,ip) in [("https://api.ipify.org",true),("https://www.google.com/generate_204",false),("https://github.com",false)] {self.check_cancel()?;checks.push(self.probe(url,ip).unwrap_or_else(|e|json!({"host":UrlHost::of(url),"status":"error","error":e})));}
        self.state.lock().unwrap_or_else(|e|e.into_inner()).checks=checks.clone();Ok(json!({"checks":checks}))
    }
    fn subscribe(&self,url:Option<&str>)->Result<Value,String>{
        {let s=self.state.lock().unwrap_or_else(|e|e.into_inner());if s.engine.is_some(){return Err("Обновлять профили можно после отключения новой версии".into());}}
        let link=url.map(str::to_owned).unwrap_or_else(||self.state.lock().unwrap_or_else(|e|e.into_inner()).profiles.subscription_url.clone());
        let u=url::Url::parse(&link).map_err(|_|"Укажите HTTPS-ссылку подписки")?;
        if u.scheme()!="https"||!u.username().is_empty()||u.password().is_some()||u.host_str().is_none()||link.len()>4096{return Err("Нужна HTTPS-ссылка без логина и пароля в адресе".into());}
        self.log.register([link.clone()]);self.check_cancel()?;
        let response=http_client(false)?.get(&link).send().map_err(|_|"Не удалось загрузить подписку по HTTPS")?;
        if !response.status().is_success(){return Err(format!("Подписка вернула HTTP {}",response.status().as_u16()));}
        let mut body=String::new();response.take(1_048_577).read_to_string(&mut body).map_err(|_|"Подписка должна быть UTF-8")?;
        let profiles=parse_subscription(&body)?;self.check_cancel()?;self.add_profiles(profiles,Some(link))?;Ok(self.public_profiles())
    }
    pub fn stop_engine(&self){
        let engine={self.state.lock().unwrap_or_else(|e|e.into_inner()).engine.take()};
        if let Some(mut e)=engine{let _=e.child.kill();let _=e.child.wait();drop(e);self.log.event("info","ENGINE_STOPPED","Остановлен только движок Corepass Next");}
        if let Err(e)=platform::restore_proxy(&self.data){self.log.event("error","CP-RECOVERY",&e);}
        let _=fs::remove_file(self.data.join("runtime.json"));
        let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.started=None;s.status="disconnected".into();s.message="Готов к подключению".into();s.exit_ip=None;s.latency=None;s.upload_rate=0;s.download_rate=0;
    }
    pub fn shutdown(&self){self.closing.store(true,Ordering::SeqCst);self.cancel.store(true,Ordering::SeqCst);self.stop_engine();}
    pub fn minimize_to_tray(&self)->bool{self.state.lock().unwrap_or_else(|e|e.into_inner()).settings.minimize_to_tray}
    fn monitor(&self){
        if self.busy.load(Ordering::SeqCst){return;}
        let dead={let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.engine.as_mut().map(|e|e.child.try_wait().map(|v|v.is_some()).unwrap_or(true)).unwrap_or(false)};
        if dead{self.stop_engine();let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());s.status="error".into();s.message="Движок остановился. Откройте журнал".into();self.log.event("error","CP-ENGINE-EXIT","Движок неожиданно завершился; настройки прокси восстановлены");}
    }
    fn metrics(&self,secret:String){
        let client=match http_client(false){Ok(c)=>c,Err(_)=>return};let response=match client.get(format!("http://127.0.0.1:{METRICS_PORT}/traffic")).bearer_auth(secret).send(){Ok(r)=>r,Err(_)=>return};
        let mut last=Instant::now();for line in BufReader::new(response).lines().map_while(Result::ok){if self.closing.load(Ordering::SeqCst){break;}if let Ok(v)=serde_json::from_str::<Value>(&line){let mut s=self.state.lock().unwrap_or_else(|e|e.into_inner());if s.engine.is_none(){break;}
            // Clash /traffic reports byte rates, not lifetime counters.
            let seconds=last.elapsed().as_secs_f64();last=Instant::now();s.download_rate=v["down"].as_u64().unwrap_or(0);s.upload_rate=v["up"].as_u64().unwrap_or(0);s.download+=(s.download_rate as f64*seconds)as u64;s.upload+=(s.upload_rate as f64*seconds)as u64;
            let sample=json!({"down":s.download_rate,"up":s.upload_rate});s.history.push(sample);if s.history.len()>90{s.history.remove(0);}
        }}
    }
}
struct UrlHost;
impl UrlHost{fn of(s:&str)->String{url::Url::parse(s).ok().and_then(|u|u.host_str().map(str::to_owned)).unwrap_or_default()}}

pub fn constant_equal(a:&str,b:&str)->bool{a.len()==b.len()&&a.bytes().zip(b.bytes()).fold(0u8,|sum,(a,b)|sum|(a^b))==0}
pub fn api_schema()->Value {
    let mut paths=serde_json::Map::new();for (path,method,summary) in [
        ("/health","get","Версия"),("/v1/status","get","Состояние соединения"),("/v1/profiles","get","Профили без паролей"),("/v1/logs","get","Очищенный журнал"),("/v1/settings","get","Настройки"),("/v1/operations/{id}","get","Результат операции"),
        ("/v1/connect","post","Подключение: mode system|tun|proxy"),("/v1/disconnect","post","Отключение"),("/v1/cancel","post","Отмена операции"),("/v1/diagnostics","post","Проверка HTTPS-маршрута"),("/v1/profiles/import","post","Импорт: uri"),("/v1/profiles/subscribe","post","Подписка: url"),("/v1/profiles/refresh","post","Обновление подписки"),("/v1/profiles/migrate","post","Копирование старых профилей"),("/v1/profiles/select","post","Выбор: profile_id"),("/v1/settings","post","Настройки: mode, minimize_to_tray"),("/v1/support/export","post","Сохранить отчёт без ключей"),("/v1/window/show","post","Показать окно")]{
        let entry=paths.entry(path).or_insert(json!({}));entry[method]=json!({"summary":summary,"responses":{"200":{"description":"JSON"},"202":{"description":"Асинхронная операция"},"409":{"description":"Занято или работает старый клиент"}},"security":if path=="/health"{json!([])}else{json!([{"localToken":[]}])}});
    }json!({"openapi":"3.0.3","info":{"title":"Corepass Next Local API","version":VERSION},"servers":[{"url":format!("http://127.0.0.1:{API_PORT}")}],"paths":paths,"components":{"securitySchemes":{"localToken":{"type":"http","scheme":"bearer"}}}})
}
pub fn start_api(app:Arc<App>)->Result<(),String>{
    let server=tiny_http::Server::http(("127.0.0.1",API_PORT)).map_err(|_|"Corepass Next уже запущен или порт API занят")?;
    std::thread::spawn(move||{while !app.closing.load(Ordering::SeqCst){let request=match server.recv_timeout(Duration::from_millis(250)){Ok(Some(r))=>r,_=>continue};let a=app.clone();std::thread::spawn(move||serve_request(a,request));}});Ok(())
}
fn serve_request(app:Arc<App>,mut req:tiny_http::Request){
    let header=|name:&str|req.headers().iter().find(|h|h.field.equiv(name)).map(|h|h.value.as_str().to_owned());
    let result:ApiResult=if header("Host")!=Some(format!("127.0.0.1:{API_PORT}"))||header("Origin").is_some()||header("Sec-Fetch-Site").is_some(){Err(error(403,"CP-LOCAL-ONLY","API доступен только локальным приложениям"))}
    else if req.url()!="/health"&&!constant_equal(&header("Authorization").unwrap_or_default(),&format!("Bearer {}",app.token)){Err(error(401,"CP-AUTH","Требуется локальный Bearer-токен"))}
    else if req.body_length().unwrap_or(0)>16_384{Err(error(413,"CP-SIZE","Максимум 16 КБ"))}
    else {let method=req.method().as_str().to_string();let path=req.url().to_string();if method=="POST"{
        if header("Content-Type").unwrap_or_default().split(';').next()!=Some("application/json"){Err(error(415,"CP-JSON","Требуется application/json"))}
        else{let mut text=String::new();if req.as_reader().take(16_385).read_to_string(&mut text).is_err()||text.len()>16_384{Err(error(400,"CP-JSON","Некорректный запрос"))}else{match serde_json::from_str::<Value>(&text){Ok(body)=>app.request(&method,&path,body),Err(_)=>Err(error(400,"CP-JSON","Нужен JSON-объект"))}}}
    }else{app.request(&method,&path,json!({}))}};
    let (code,body)=match result{Ok(v)=>(if v.get("operation_id").is_some(){202}else{200},v),Err(e)=>(e.status,json!({"error":{"code":e.code,"message":app.log.redact(&e.message)}}))};
    let mut response=tiny_http::Response::from_string(body.to_string()).with_status_code(code);
    for (name,value) in [("Content-Type","application/json; charset=utf-8"),("Cache-Control","no-store"),("X-Content-Type-Options","nosniff")] {response.add_header(tiny_http::Header::from_bytes(name,value).unwrap());}
    let _=req.respond(response);
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn auth_compares_exactly(){assert!(constant_equal("a","a"));assert!(!constant_equal("a","b"));assert!(!constant_equal("a","aa"));}
    #[test] fn disconnected_commands_cannot_control_processes(){let dir=tempfile::tempdir().unwrap();let app=App::new(dir.path().join("next"),dir.path().join("missing-engine")).unwrap();
        assert!(app.request("POST","/v1/exec",json!({"command":"anything"})).is_err());assert!(app.request("POST","/v1/disconnect",json!({"pid":123})).is_err());
        app.request("POST","/v1/profiles/import",json!({"uri":"naive+https://user:pass@example.com"})).unwrap();let report=app.request("POST","/v1/support/export",json!({})).unwrap();
        let text=fs::read_to_string(report["path"].as_str().unwrap()).unwrap();assert!(!text.contains("naive+https"));assert!(!text.contains(&app.token));app.shutdown();
    }
}
