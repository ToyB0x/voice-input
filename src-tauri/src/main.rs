#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(any(target_os = "macos", test))]
mod audio_buffer;
#[cfg(target_os = "macos")]
mod recorder;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder =
        builder
            .manage(recorder::Recorder::new())
            .invoke_handler(tauri::generate_handler![
                recorder::recording_status,
                recorder::request_microphone_permission,
                recorder::start_recording,
                recorder::stop_recording,
                recorder::clear_recording,
                recorder::open_microphone_settings,
            ]);
    builder
        .run(tauri::generate_context!())
        .expect("failed to run Voice Input");
}
