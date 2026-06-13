mod commands;

use std::sync::Mutex;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(commands::WatcherState::default()))
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::open_session,
            commands::stop_watching,
            commands::search_transcripts,
            commands::open_for_edit,
            commands::commit_session,
            commands::fork_session,
            commands::delete_session,
            commands::rename_session,
            commands::delete_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
