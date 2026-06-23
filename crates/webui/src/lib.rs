mod commands;
mod shared;
mod terminal;
mod window;

use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, WindowEvent};

/// Build the application menu: the standard macOS App/Edit/Window items
/// (installing a custom menu replaces the OS default, so the standard
/// clipboard/edit shortcuts must be re-declared) plus a custom New Window item.
fn build_menu(handle: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app_menu = Submenu::with_items(
        handle,
        "App",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::services(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::show_all(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
        ],
    )?;

    let new_window =
        MenuItem::with_id(handle, "new_window", "New Window", true, Some("CmdOrCtrl+N"))?;
    let file_menu = Submenu::with_items(
        handle,
        "File",
        true,
        &[&new_window, &PredefinedMenuItem::close_window(handle, None)?],
    )?;

    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(handle, None)?,
            &PredefinedMenuItem::fullscreen(handle, None)?,
        ],
    )?;

    Menu::with_items(handle, &[&app_menu, &file_menu, &edit_menu, &window_menu])
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(commands::WatcherState::default()))
        .manage(Mutex::new(terminal::TerminalState::default()))
        .manage(shared::Shared::new())
        .setup(|app| {
            // One app-wide poller: every 3s it refreshes the shared activity scan
            // and (only when the project dir's signature changes) re-discovers,
            // then broadcasts to ALL windows. Replaces each window polling on its
            // own, so N windows cost the same as one. Idle when no windows exist.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(3));
                if handle.webview_windows().is_empty() {
                    continue;
                }
                let root = match claude_manager_core::store::paths::default_claude_root() {
                    Some(r) => r,
                    None => continue,
                };
                let outcome = handle.state::<shared::Shared>().poll(&root);
                let _ = handle.emit("activity-updated", &outcome.activity);
                if outcome.sessions_changed {
                    let _ = handle.emit("sessions-changed", ());
                }
            });
            Ok(())
        })
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "new_window" {
                window::spawn_window(app);
            }
        })
        .on_window_event(|win, event| {
            // When a window is destroyed, drop its live-tail watcher (the tail
            // thread exits when the channel sender is dropped). A backstop for
            // hard closes where the frontend's stop_watching may not run.
            if let WindowEvent::Destroyed = event {
                if let Some(state) = win.try_state::<Mutex<commands::WatcherState>>() {
                    if let Ok(mut guard) = state.lock() {
                        guard.watchers.remove(win.label());
                    }
                }
                if let Some(ts) = win.try_state::<Mutex<terminal::TerminalState>>() {
                    if let Ok(mut guard) = ts.lock() {
                        guard.close_window(win.label());
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::refresh_sessions,
            commands::open_session,
            commands::stop_watching,
            commands::search_transcripts,
            commands::open_for_edit,
            commands::commit_session,
            commands::fork_session,
            commands::delete_session,
            commands::rename_session,
            commands::delete_message,
            commands::new_session,
            commands::path_exists,
            commands::session_activity,
            commands::focus_window,
            commands::terminal_open,
            commands::terminal_input,
            commands::terminal_resize,
            commands::terminal_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Backstop: when the app exits by any path Tauri runs its loop for
            // (Cmd-Q, menu Quit, last window closed, bundled .app), kill every
            // terminal PTY so no claude child is orphaned. (Per-window Destroyed
            // cleanup still handles closing a single window.)
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<Mutex<terminal::TerminalState>>() {
                    if let Ok(mut guard) = state.lock() {
                        guard.close_all();
                    }
                }
            }
        });
}
