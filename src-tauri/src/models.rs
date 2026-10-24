//! Narrow IPC: bundled model IDs and explicit setup, never user content or URLs.
use local_models::{Manager, Model, Status};
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
    models: tauri::State<'_, Manager>,
    ids: Vec<String>,
    download: bool,
) -> Result<Status, String> {
    models.start(ids, download).map_err(str::to_owned)
}
#[tauri::command]
pub fn cancel_model_preparation(models: tauri::State<'_, Manager>) {
    models.cancel();
}
