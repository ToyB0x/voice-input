//! Serialize operation startup so user tests and updates cannot race normal work.
#[derive(Clone, Default)]
pub struct Activity {
    gate: std::sync::Arc<std::sync::Mutex<()>>,
    closing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Activity {
    pub fn begin_shutdown(&self) {
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn is_closing(&self) -> bool {
        self.closing.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn wait_for_idle(&self) {
        drop(self.gate.lock().unwrap_or_else(|error| error.into_inner()));
    }
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        if self.is_closing() {
            return Err("アプリを終了しています。".into());
        }
        let guard = self
            .gate
            .lock()
            .map_err(|_| "処理状態を確認できません。アプリを再起動してください。".to_owned())?;
        if self.is_closing() {
            return Err("アプリを終了しています。".into());
        }
        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_rejects_new_work_and_waits_for_the_existing_action() {
        let activity = Activity::default();
        let current = activity.lock().unwrap();
        activity.begin_shutdown();
        assert!(activity.lock().is_err());
        let owner = activity.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            owner.wait_for_idle();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        drop(current);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert!(activity.lock().is_err());
    }
}
