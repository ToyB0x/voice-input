//! Opt-in fixed stage markers only; never log audio, paths or recognized text.
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};
use tauri::Manager;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

static NATIVE_APP: Mutex<Option<tauri::AppHandle>> = Mutex::new(None);

// AppKit's native Quit invokes applicationWillTerminate directly in the current
// Tao implementation, without Tauri's cancellable ExitRequested event. Install
// the public applicationShouldTerminate: delegate hook when Tao provides none.
pub(crate) fn install_native_quit_hook(app: &tauri::AppHandle) -> Result<(), String> {
    let _main =
        objc2::MainThreadMarker::new().ok_or("終了フックはメインスレッドで設定してください。")?;
    // SAFETY: NSApplication exists after Tauri creates its macOS runtime; this
    // setup callback runs on AppKit's main thread. Retain the existing delegate
    // while refreshing its public delegate registration; do not replace its class.
    unsafe {
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![application, delegate];
        let delegate =
            Retained::retain(delegate).ok_or("アプリの終了デリゲートを確認できません。")?;
        let class = delegate.class();
        let selector = sel!(applicationShouldTerminate:);
        if class.instance_method(selector).is_some() {
            diagnostic("native-quit-delegate-existing");
            return Ok(());
        }
        // NSUInteger (NSApplicationTerminateReply), id, SEL, NSApplication*.
        // Both supported macOS architectures use 64-bit NSUInteger encoding Q.
        let implementation: objc2::runtime::Imp = std::mem::transmute(
            native_should_terminate
                as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
        );
        if !objc2::ffi::class_addMethod(
            class as *const _ as *mut _,
            selector,
            implementation,
            c"Q@:@".as_ptr(),
        )
        .as_bool()
        {
            return Err("アプリの終了フックを登録できません。".into());
        }
        *NATIVE_APP.lock().unwrap_or_else(|error| error.into_inner()) = Some(app.clone());
        // AppKit caches optional delegate selectors at setDelegate:. Keep Tao's
        // original object and all its existing methods/ivars intact.
        let _: () = msg_send![application, setDelegate: None::<&AnyObject>];
        let _: () = msg_send![application, setDelegate: Some(&*delegate)];
    }
    diagnostic("native-quit-hook-installed");
    Ok(())
}

unsafe extern "C-unwind" fn native_should_terminate(
    _: *mut AnyObject,
    _: Sel,
    _: *mut AnyObject,
) -> usize {
    let app = NATIVE_APP
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let Some(app) = app else { return 1 }; // NSTerminateNow after native exit.
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        if lifecycle.ready() {
            lifecycle.join_worker();
            return 1;
        }
    }
    // NSTerminateCancel keeps AppKit running while the ordinary Tauri exit
    // request completes resource shutdown. Repeated requests are coalesced below.
    app.exit(0);
    0
}

pub(crate) fn native_exit() {
    *NATIVE_APP.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

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
    fn join_worker(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

pub(crate) fn request_exit(app: &tauri::AppHandle, api: tauri::ExitRequestApi, code: Option<i32>) {
    let lifecycle = app.state::<Lifecycle>();
    if lifecycle.ready() {
        lifecycle.join_worker();
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
