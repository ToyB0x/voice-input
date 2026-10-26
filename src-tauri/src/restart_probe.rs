//! Only compiled with ci-restart-probe. Normal/release builds have no probe hook.
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::Manager;

fn options() -> Option<(PathBuf, bool)> {
    let args: Vec<_> = std::env::args_os().collect();
    let index = args.iter().position(|arg| arg == "--ci-restart-probe")?;
    Some((
        PathBuf::from(args.get(index + 1)?),
        args.get(index + 2)? != "none",
    ))
}

pub(crate) fn diagnostic(stage: &str) {
    if let Some((root, _)) = options()
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(format!("process-{}.log", std::process::id())))
    {
        let _ = writeln!(file, "{stage}");
    }
}

pub(crate) fn setup(app: &tauri::AppHandle) {
    let Some((root, needs_model)) = options() else {
        return;
    };
    let relaunched = root.join("initial.json").exists();
    let path = root.join(if relaunched {
        "relaunched.json"
    } else {
        "initial.json"
    });
    std::fs::write(
        path,
        serde_json::json!({"pid": std::process::id()}).to_string(),
    )
    .unwrap();
    let handle = app.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(90);
        while needs_model
            && !handle
                .state::<crate::recorder::Recorder>()
                .status()
                .is_ok_and(|status| status.recognition.model_ready)
        {
            if Instant::now() >= deadline {
                std::fs::write(root.join("error.txt"), "Model readiness timed out").unwrap();
                handle.exit(1);
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if relaunched {
            std::fs::write(root.join("relaunched-ready"), b"ready").unwrap();
        } else {
            diagnostic("probe-request-restart");
            // Exactly the production post-install restart path, with real owners.
            crate::lifecycle::request_restart(&handle);
        }
    });
}
