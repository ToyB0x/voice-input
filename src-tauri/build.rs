fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "model_catalog",
        "model_preparation_status",
        "prepare_models",
        "cancel_model_preparation",
        "recording_status",
        "request_microphone_permission",
        "start_recording",
        "stop_recording",
        "clear_recording",
        "open_microphone_settings",
        "load_model",
        "transcribe_recording",
        "copy_transcript",
        "desktop_status",
        "save_preferences",
        "insertion_status",
        "context_status",
        "request_screen_permission",
        "correct_transcript",
        "correction_status",
        "copy_correction",
        "request_accessibility",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application");
}
