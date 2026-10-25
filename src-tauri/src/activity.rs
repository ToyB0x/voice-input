//! Serialize operation startup so user tests and updates cannot race normal work.
#[derive(Clone, Default)]
pub struct Activity(std::sync::Arc<std::sync::Mutex<()>>);
impl Activity {
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.0
            .lock()
            .map_err(|_| "処理状態を確認できません。アプリを再起動してください。".into())
    }
}
