//! Narrow IPC: bundled model IDs and explicit setup, never user content or URLs.
use local_models::{Manager, Model, Status};
use tauri::Manager as _;
#[tauri::command]
pub fn model_catalog(models: tauri::State<'_, Manager>) -> Vec<Model> {
    models.models()
}
#[tauri::command]
pub fn model_preparation_status(models: tauri::State<'_, Manager>) -> Status {
    models.status()
}
#[tauri::command]
pub fn prepare_models(
    app: tauri::AppHandle,
    models: tauri::State<'_, Manager>,
    ids: Vec<String>,
    download: bool,
) -> Result<Status, String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    if app.state::<crate::comparison::Comparisons>().working()
        || app.state::<crate::updater::Updates>().installing()
    {
        return Err("比較・更新の終了をお待ちください。".into());
    }
    models.start(ids, download).map_err(str::to_owned)
}
#[tauri::command]
pub fn cancel_model_preparation(models: tauri::State<'_, Manager>) {
    models.cancel();
}
