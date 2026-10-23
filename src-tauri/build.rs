fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "recording_status",
        "request_microphone_permission",
        "start_recording",
        "stop_recording",
        "clear_recording",
        "open_microphone_settings",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application");
}
