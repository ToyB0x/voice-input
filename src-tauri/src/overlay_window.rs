//! Public AppKit window ordering only: never activate the app or make a key window.
use objc2::{msg_send, runtime::AnyObject};
use tauri::WebviewWindow;

pub(crate) fn configure(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_focusable(false)?;
    window.set_ignore_cursor_events(true)?;
    Ok(())
}
pub(crate) fn position(window: &WebviewWindow) -> tauri::Result<()> {
    let monitor = window
        .primary_monitor()?
        .ok_or_else(|| tauri::Error::AssetNotFound("primary monitor".into()))?;
    let area = monitor.work_area();
    let size = window.outer_size()?;
    let left = i64::from(area.position.x)
        + (i64::from(area.size.width) - i64::from(size.width)).max(0) / 2;
    let top = i64::from(area.position.y)
        + (i64::from(area.size.height) - i64::from(size.height) - 24).max(0);
    window.set_position(tauri::PhysicalPosition::new(left as i32, top as i32))?;
    Ok(())
}
pub(crate) fn show(window: &WebviewWindow) -> tauri::Result<()> {
    position(window)?;
    // SAFETY: callers dispatch to AppKit's main thread; Tauri owns this NSWindow.
    // orderFrontRegardless displays an inactive application's window without
    // makeKeyAndOrderFront / activateIgnoringOtherApps (Tao's ordinary show path).
    unsafe {
        let native = window.ns_window()?.cast::<AnyObject>();
        let _: () = msg_send![native, orderFrontRegardless];
    }
    Ok(())
}
pub(crate) fn hide(window: &WebviewWindow) -> tauri::Result<()> {
    // SAFETY: same main-thread / ownership contract as show.
    unsafe {
        let native = window.ns_window()?.cast::<AnyObject>();
        let _: () = msg_send![native,orderOut:std::ptr::null::<AnyObject>()];
    }
    Ok(())
}
