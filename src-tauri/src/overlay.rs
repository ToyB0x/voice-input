//! Cache only status metadata; no audio, transcripts, context or target identifiers.
use crate::overlay_state::{Signal, Status, Tracker};
use std::{sync::Mutex, time::Instant};
use tauri::Manager;
struct Inner {
    tracker: Tracker,
    status: Status,
}
pub(crate) struct Overlay {
    inner: Mutex<Inner>,
    started: Instant,
}
impl Default for Overlay {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                tracker: Tracker::default(),
                status: Status::default(),
            }),
            started: Instant::now(),
        }
    }
}
impl Overlay {
    pub fn tick(&self, app: &tauri::AppHandle, enabled: bool, signal: &Signal, delivery: &str) {
        let changed = if let Ok(mut inner) = self.inner.lock() {
            let next = inner.tracker.tick(
                enabled,
                signal,
                delivery,
                self.started.elapsed().as_millis() as u64,
            );
            let changed = next.visible != inner.status.visible;
            inner.status = next;
            changed
        } else {
            false
        };
        if changed {
            let next = app.clone();
            let _ = app.run_on_main_thread(move || {
                // Read current intent here, not a stale queued show request.
                let visible = next.state::<Overlay>().status().visible
                    && !next.state::<crate::activity::Activity>().is_closing();
                if let Some(window) = next.get_webview_window("overlay") {
                    let result = if visible {
                        crate::overlay_window::show(&window)
                    } else {
                        crate::overlay_window::hide(&window)
                    };
                    if result.is_err() {
                        let _ = crate::overlay_window::hide(&window);
                    }
                }
            });
        }
    }
    fn status(&self) -> Status {
        self.inner
            .lock()
            .map(|inner| inner.status.clone())
            .unwrap_or_default()
    }
}
#[tauri::command]
pub(crate) fn overlay_status(overlay: tauri::State<'_, Overlay>) -> Status {
    overlay.status()
}
pub(crate) fn setup(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window missing")?;
    crate::overlay_window::configure(&window)?;
    Ok(())
}
