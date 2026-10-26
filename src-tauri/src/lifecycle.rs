//! Opt-in fixed stage markers only; never log audio, paths or recognized text.
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Default)]
pub(crate) struct Lifecycle {
    phase: AtomicU8,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl Lifecycle {
    fn begin(&self) -> bool {
        self.phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn ready(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 2
    }
    fn finish(&self) {
        self.phase.store(2, Ordering::Release);
    }
}

pub(crate) fn request_exit(app: &tauri::AppHandle, api: tauri::ExitRequestApi, code: Option<i32>) {
    let lifecycle = app.state::<Lifecycle>();
    if lifecycle.ready() {
        if let Some(worker) = lifecycle
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = worker.join();
        }
        diagnostic("exit-ready");
        return;
    }
    api.prevent_exit();
    if !lifecycle.begin() {
        return;
    }
    // Stop accepting work immediately, then keep AppKit's main loop alive while
    // native owners finish. Joining a tray worker on the main thread can deadlock
    // because its final UI update may itself be waiting for the main thread.
    app.state::<crate::activity::Activity>().begin_shutdown();
    let _ = app.global_shortcut().unregister_all();
    let handle = app.clone();
    let worker = std::thread::spawn(move || {
        diagnostic("exit-cleanup-started");
        handle.state::<crate::activity::Activity>().wait_for_idle();
        handle.state::<crate::insertion::Inserter>().cancel();
        handle.state::<crate::context::Reader>().cancel();
        handle
            .state::<crate::comparison::Comparisons>()
            .cancel(true);
        handle.state::<local_models::Manager>().cancel();
        handle.state::<crate::correction::Corrector>().cancel();
        handle.state::<crate::desktop::Desktop>().shutdown();
        handle.state::<crate::recorder::Recorder>().shutdown();
        handle.state::<crate::comparison::Comparisons>().shutdown();
        handle.state::<local_models::Manager>().shutdown();
        handle.state::<crate::correction::Corrector>().shutdown();
        diagnostic("exit-cleanup-completed");
        handle.state::<Lifecycle>().finish();
        let next = handle.clone();
        if handle
            .run_on_main_thread(move || next.exit(code.unwrap_or(0)))
            .is_err()
        {
            diagnostic("exit-dispatch-failed");
        }
    });
    *lifecycle
        .worker
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(worker);
}

pub(crate) fn diagnostic(stage: &'static str) {
    if std::env::var_os("VOICE_INPUT_LIFECYCLE_DIAGNOSTICS").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        use std::io::Write;
        // A closed launcher pipe must never turn normal exit into a panic.
        let _ = writeln!(std::io::stderr().lock(), "voice-input:lifecycle:{stage}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_exit_requests_start_only_one_cleanup_and_wait_until_ready() {
        let lifecycle = Lifecycle::default();
        assert!(!lifecycle.ready());
        assert!(lifecycle.begin());
        assert!(!lifecycle.begin());
        assert!(!lifecycle.ready());
        lifecycle.finish();
        assert!(lifecycle.ready());
        assert!(!lifecycle.begin());
    }
}
