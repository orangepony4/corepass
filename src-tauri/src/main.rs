#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use corepass_core::{App,platform};
use std::{path::PathBuf,sync::{Arc,atomic::Ordering}};
use tauri::{Manager,menu::{Menu,MenuItem},tray::{TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState}};

#[tauri::command]
async fn api_request(app:tauri::State<'_,Arc<App>>,method:String,path:String,body:serde_json::Value)->Result<serde_json::Value,serde_json::Value>{
    let a=app.inner().clone();
    tauri::async_runtime::spawn_blocking(move||a.request(&method,&path,body).map_err(|e|serde_json::json!({"code":e.code,"message":e.message})))
        .await.map_err(|_|serde_json::json!({"code":"CP-WORKER","message":"Ошибка потока управления"}))?
}
fn main(){
    let args:Vec<String>=std::env::args().collect();
    if args.get(1).map(String::as_str)==Some("--guard"){
        if let (Some(pid),Some(data))=(args.get(2).and_then(|s|s.parse::<u32>().ok()),args.get(3)){
            platform::wait_parent(pid);let dir=PathBuf::from(data);let _=platform::restore_proxy(&dir);
        }return;
    }
    let data=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("CorepassNext");
    let exe=std::env::current_exe().expect("Application executable location");
    let engine=exe.parent().unwrap().join("engine/sing-box.exe");
    let control=match App::new(data,engine){Ok(a)=>a,Err(e)=>{eprintln!("{e}");return;}};
    let logger=control.log.clone();std::panic::set_hook(Box::new(move|info|logger.event("error","CP-PANIC",&format!("{info}\n{}",std::backtrace::Backtrace::force_capture()))));
    if let Err(e)=corepass_core::start_api(control.clone()){control.log.event("error","CP-API-BIND",&e);return;}
    let _=platform::quiet(std::process::Command::new(&exe).arg("--guard").arg(std::process::id().to_string()).arg(&control.data).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())).spawn();
    let a=control.clone();
    let result=tauri::Builder::default().manage(control.clone()).invoke_handler(tauri::generate_handler![api_request])
        .setup(move|app|{
            let show=MenuItem::with_id(app,"show","Открыть Corepass",true,None::<&str>)?;
            let quit=MenuItem::with_id(app,"quit","Выйти из новой версии",true,None::<&str>)?;
            let menu=Menu::with_items(app,&[&show,&quit])?;
            let mut tray=TrayIconBuilder::new().tooltip("Corepass Next").menu(&menu).show_menu_on_left_click(false)
                .on_menu_event(|app,event|match event.id.as_ref(){"show"=>{if let Some(w)=app.get_webview_window("main"){let _=w.show();let _=w.set_focus();}},"quit"=>{app.state::<Arc<App>>().shutdown();app.exit(0);},_=>()})
                .on_tray_icon_event(|tray,event|{if let TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}=event{if let Some(w)=tray.app_handle().get_webview_window("main"){let _=w.show();let _=w.set_focus();}}});
            if let Some(icon)=app.default_window_icon(){tray=tray.icon(icon.clone());}tray.build(app)?;
            let handle=app.handle().clone();let a=a.clone();std::thread::spawn(move||{while !a.closing.load(Ordering::SeqCst){std::thread::sleep(std::time::Duration::from_millis(400));if a.show_requested.swap(false,Ordering::SeqCst){if let Some(w)=handle.get_webview_window("main"){let _=w.show();let _=w.set_focus();}}}});
            Ok(())
        })
        .on_window_event(|window,event|{if let tauri::WindowEvent::CloseRequested{api,..}=event{
            let a=window.state::<Arc<App>>();if a.minimize_to_tray(){api.prevent_close();let _=window.hide();}else{a.shutdown();}
        }})
        .build(tauri::generate_context!());
    match result{Ok(app)=>app.run(move|_,event|if let tauri::RunEvent::Exit=event{control.shutdown();}),Err(e)=>{control.log.event("error","CP-UI-BOOT",&e.to_string());control.shutdown();}}
}
