//! Cache only fixed status metadata; native drawing is bounded to one queued task.
use crate::overlay_state::{Signal, Status, Tracker};
use std::{sync::Mutex, time::Instant};
use tauri::Manager;
struct Inner {
    tracker: Tracker,
    status: Status,
    pending: bool,
    applied_visible: bool,
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
                pending: false,
                applied_visible: false,
            }),
            started: Instant::now(),
        }
    }
}
impl Overlay {
    pub fn tick(&self, app: &tauri::AppHandle, enabled: bool, signal: &Signal, delivery: &str) {
        let schedule = if let Ok(mut inner) = self.inner.lock() {
            let next = inner.tracker.tick(
                enabled,
                signal,
                delivery,
                self.started.elapsed().as_millis() as u64,
            );
            let needs_paint = next.visible || inner.status.visible || inner.applied_visible;
            inner.status = next;
            if needs_paint && !inner.pending {
                inner.pending = true;
                true
            } else {
                false
            }
        } else {
            false
        };
        if schedule {
            let next = app.clone();
            let _ = app.run_on_main_thread(move || {
                let status = if let Ok(mut inner) = next.state::<Overlay>().inner.lock() {
                    let mut status = inner.status.clone();
                    status.visible &= !next.state::<crate::activity::Activity>().is_closing();
                    inner.pending = false;
                    inner.applied_visible = status.visible;
                    status
                } else {
                    Status::default()
                };
                if crate::overlay_window::render(&status).is_err() {
                    let _ = crate::overlay_window::hide();
                }
            });
        }
    }
}
pub(crate) fn setup() -> Result<(), Box<dyn std::error::Error>> {
    crate::overlay_window::initialize().map_err(Into::into)
}
