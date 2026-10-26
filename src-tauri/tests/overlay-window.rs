//! Native window regression, independent of WebView rendering and TCC grants.
#[cfg(target_os = "macos")]
#[path = "../src/overlay_window.rs"]
mod overlay_window;
#[cfg(target_os = "macos")]
mod probe {
    use objc2::{
        class, msg_send,
        runtime::{AnyObject, Bool},
    };
    use std::{
        path::PathBuf,
        process::Command,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use tauri::Manager;
    fn on_main<T: Send + 'static>(
        app: &tauri::AppHandle,
        task: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        let handle = app.clone();
        app.run_on_main_thread(move || {
            let _ = tx.send(task(&handle));
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "native main-thread operation timed out".to_owned())?
    }
    fn front_pid() -> i32 {
        assert!(objc2::MainThreadMarker::new().is_some());
        unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
            if front.is_null() {
                0
            } else {
                msg_send![front, processIdentifier]
            }
        }
    }
    fn inspect(app: &tauri::AppHandle, visible: bool) -> Result<(), String> {
        let window = app.get_webview_window("overlay").ok_or("overlay missing")?;
        unsafe {
            let native = window
                .ns_window()
                .map_err(|e| e.to_string())?
                .cast::<AnyObject>();
            let key: Bool = msg_send![native, isKeyWindow];
            let can_key: Bool = msg_send![native, canBecomeKeyWindow];
            let can_main: Bool = msg_send![native, canBecomeMainWindow];
            let ignoring: Bool = msg_send![native, ignoresMouseEvents];
            let shown: Bool = msg_send![native, isVisible];
            let level: isize = msg_send![native, level];
            if key.as_bool()
                || can_key.as_bool()
                || can_main.as_bool()
                || !ignoring.as_bool()
                || shown.as_bool() != visible
                || level < 3
            {
                return Err(
                    "overlay focus, mouse, visibility or floating-level property mismatch".into(),
                );
            }
        }
        Ok(())
    }
    fn run(app: &tauri::AppHandle, fixture: PathBuf) -> Result<(), String> {
        on_main(app, |app| inspect(app, false))?;
        let marker = std::env::temp_dir().join(format!("overlay-focus-{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let launched = Command::new("/usr/bin/open")
            .arg("-n")
            .arg("-a")
            .arg(fixture)
            .arg("--args")
            .arg(&marker)
            .status()
            .map_err(|e| e.to_string())?;
        if !launched.success() {
            return Err("focus fixture launch failed".into());
        }
        let start = Instant::now();
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&marker)
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
            {
                break pid;
            }
            if start.elapsed() > Duration::from_secs(15) {
                return Err("focus fixture did not start".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let outcome = (|| {
            let start = Instant::now();
            loop {
                if on_main(app, move |_| Ok(front_pid()))? == pid {
                    break;
                }
                if start.elapsed() > Duration::from_secs(10) {
                    return Err("focus fixture did not become frontmost".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            for visible in [true, false, true, false] {
                on_main(app, move |app| {
                    let window = app.get_webview_window("overlay").ok_or("overlay missing")?;
                    if visible {
                        super::overlay_window::show(&window)
                    } else {
                        super::overlay_window::hide(&window)
                    }
                    .map_err(|e| e.to_string())
                })?;
                std::thread::sleep(Duration::from_millis(250));
                on_main(app, move |app| {
                    inspect(app, visible)?;
                    if front_pid() != pid {
                        return Err("overlay changed the frontmost application".into());
                    }
                    Ok(())
                })?;
            }
            Ok(())
        })();
        // Dedicated synthetic app only; no user application or content is touched.
        let _ = on_main(app, move |_| {
            unsafe {
                let target: *mut AnyObject = msg_send![class!(NSRunningApplication),runningApplicationWithProcessIdentifier:pid];
                if !target.is_null() {
                    let _: Bool = msg_send![target, terminate];
                }
            }
            Ok(())
        });
        let _ = std::fs::remove_file(marker);
        outcome
    }
    pub fn main() {
        let Some(fixture) = std::env::var_os("VOICE_INPUT_OVERLAY_FOCUS_TARGET") else {
            println!("SKIP: native overlay focus probe requires the explicit CI fixture");
            return;
        };
        let mut context = tauri::generate_context!();
        context
            .config_mut()
            .app
            .windows
            .retain(|window| window.label == "overlay");
        let app = tauri::Builder::default()
            .setup(|app| {
                let window = app.get_webview_window("overlay").ok_or("overlay missing")?;
                super::overlay_window::configure(&window)?;
                Ok(())
            })
            .build(context)
            .expect("build native overlay probe");
        let mut fixture = Some(PathBuf::from(fixture));
        app.run(move|app,event| {
            if matches!(event,tauri::RunEvent::Ready) && let Some(fixture)=fixture.take() {
                let app=app.clone();
                std::thread::spawn(move|| {
                    let outcome=run(&app,fixture);
                    match &outcome {
                        Ok(())=>println!("{{\"status\":\"passed\",\"nonFocusable\":true,\"ignoresMouse\":true,\"frontmostPreserved\":true,\"showHideCycles\":2,\"renderingTested\":false}}"),
                        Err(reason)=>eprintln!("native overlay probe failed: {reason}"),
                    }
                    app.exit(if outcome.is_ok(){0}else{1});
                });
            }
        });
    }
}
#[cfg(target_os = "macos")]
fn main() {
    probe::main();
}
#[cfg(not(target_os = "macos"))]
fn main() {}
