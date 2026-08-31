mod commands;
mod config;
mod dock_name;
mod session_meta;
mod shared;
mod status_file;
mod terminal;
mod win_id;
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
            // Unbundled dev runs would otherwise show the binary name in the
            // Dock; no-op in release bundles and off macOS.
            if let Some(name) = app.config().product_name.as_deref() {
                dock_name::set_dev_dock_name(name);
            }
            // One app-wide poller: every 3s it refreshes the shared activity scan
            // and (only when the project dir's signature changes) re-discovers,
            // then broadcasts to ALL windows. Replaces each window polling on its
            // own, so N windows cost the same as one. Idle when no windows exist.
            let handle = app.handle().clone();
            // Last published status, so the file is rewritten only on change.
            let mut last_status: Option<status_file::StatusFile> = None;
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(3));
                if handle.webview_windows().is_empty() {
                    continue;
                }
                let root = match claude_code_manager_core::store::paths::default_claude_root() {
                    Some(r) => r,
                    None => continue,
                };
                let mut outcome = handle.state::<shared::Shared>().poll(&root);

                // Which sessions ccm is running itself. Known here (the poller
                // owns no state but can reach both pools), and nowhere else:
                // the activity scan only sees files, not processes.
                let hosted_now: Vec<terminal::Hosted> = handle
                    .state::<Mutex<terminal::TerminalState>>()
                    .lock()
                    .map(|g| g.hosted())
                    .unwrap_or_default();

                // Stamp `hosted`/`busy` on the scanned rows, and mint rows for
                // hosted sessions the scan skipped — it only tail-reads files
                // touched in the last 60s, and a session mid-way through a long
                // tool call writes nothing, which is exactly when its indicator
                // must stay lit.
                //
                // `busy` is hosted AND recently talking: a PTY that exists says
                // claude is alive, not that it is working — a session parked on
                // a question is alive and silent, and used to read as "running"
                // forever.
                if !hosted_now.is_empty() {
                    let now = terminal::now_secs();
                    let busy_at: std::collections::BTreeMap<&str, i64> = hosted_now
                        .iter()
                        .map(|h| (h.session_id.as_str(), h.last_busy_at))
                        .collect();
                    let busy_ids: std::collections::BTreeSet<&str> = hosted_now
                        .iter()
                        .filter(|h| now - h.last_busy_at < terminal::BUSY_QUIET_SECS)
                        .map(|h| h.session_id.as_str())
                        .collect();
                    let hosted_ids: std::collections::BTreeSet<&str> =
                        hosted_now.iter().map(|h| h.session_id.as_str()).collect();
                    for a in outcome.activity.iter_mut() {
                        a.hosted = hosted_ids.contains(a.id.as_str());
                        a.busy = busy_ids.contains(a.id.as_str());
                        a.busy_at = busy_at.get(a.id.as_str()).copied().unwrap_or(0);
                    }
                    let have: std::collections::BTreeSet<String> =
                        outcome.activity.iter().map(|a| a.id.clone()).collect();
                    let missing: Vec<&str> = hosted_ids
                        .iter()
                        .copied()
                        .filter(|id| !have.contains(*id))
                        .collect();
                    if !missing.is_empty() {
                        let sessions =
                            handle.state::<shared::Shared>().sessions(&root).unwrap_or_default();
                        for s in sessions.iter().filter(|s| missing.contains(&s.id.to_string().as_str()))
                        {
                            let Some(at) = s.last_event_at else { continue };
                            outcome.activity.push(commands::SessionActivity {
                                id: s.id.to_string(),
                                last_event_at: at
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(0),
                                awaiting_input: s.awaiting_input,
                                hosted: true,
                                busy: busy_ids.contains(&s.id.to_string().as_str()),
                                busy_at: busy_at
                                    .get(&s.id.to_string().as_str())
                                    .copied()
                                    .unwrap_or(0),
                            });
                        }
                    }
                    handle.state::<shared::Shared>().set_activity(outcome.activity.clone());
                }

                let _ = handle.emit("activity-updated", &outcome.activity);
                if outcome.sessions_changed {
                    let _ = handle.emit("sessions-changed", ());
                }

                // Publish the status file (opt-in). Here because this thread
                // already holds the freshest activity for the whole app;
                // `publish` no-ops unless the content actually changed.
                let path = status_file::status_path();
                if config::load().integrations.publish_status {
                    let hosted: std::collections::BTreeMap<String, Option<i64>> = handle
                        .state::<Mutex<terminal::TerminalState>>()
                        .lock()
                        .map(|g| g.hosted())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|h| (h.session_id, win_id::window_id(&handle, &h.owner)))
                        .collect();
                    let sessions =
                        handle.state::<shared::Shared>().sessions(&root).unwrap_or_default();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let next = status_file::desired_state(status_file::Inputs {
                        activity: &outcome.activity,
                        sessions: &sessions,
                        hosted: &hosted,
                        last_viewed: &handle.state::<shared::Shared>().last_viewed(),
                        now,
                    });
                    if let Some(published) = status_file::publish(&path, next, last_status.as_ref())
                    {
                        last_status = Some(published);
                    }
                } else if last_status.take().is_some() {
                    // Just switched off — don't leave a snapshot behind that
                    // can no longer change.
                    status_file::unpublish(&path);
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
            commands::resolve_branch_messages,
            commands::open_session,
            commands::stop_watching,
            commands::search_messages,
            commands::fork_session,
            commands::message_previews,
            commands::delete_session,
            commands::new_session,
            commands::path_exists,
            commands::get_config,
            commands::set_config,
            commands::get_session_meta,
            commands::set_session_meta,
            commands::session_activity,
            commands::set_last_viewed,
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
                // A status file outliving the app is worse than none: consumers
                // would keep believing a snapshot that can no longer change.
                status_file::unpublish(&status_file::status_path());
            }
        });
}
