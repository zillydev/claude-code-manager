mod activity;
mod agent_bridge;
mod close_guard;
mod commands;
mod config;
mod dev_reload;
mod dock_name;
mod hooks;
mod plugin;
mod precise_status;
mod program_status;
mod session_link;
mod session_meta;
mod shared;
mod status_file;
mod terminal;
mod update_check;
mod win_id;
mod window;

use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;

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
            &MenuItem::with_id(
                handle,
                "quit",
                format!("Quit {}", handle.package_info().name),
                true,
                Some("CmdOrCtrl+Q"),
            )?,
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

    let develop_menu;
    let mut menus: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        vec![&app_menu, &file_menu, &edit_menu, &window_menu];
    if dev_reload::available() {
        develop_menu = dev_reload::menu(handle)?;
        menus.push(&develop_menu);
    }
    Menu::with_items(handle, &menus)
}

pub fn run() {
    let mode = agent_bridge::mode();
    let bridge = mode != agent_bridge::Mode::Off;
    let mut context = tauri::generate_context!();
    if bridge {
        agent_bridge::prepare(&mut context, mode);
    }
    let mut builder = tauri::Builder::default();
    if bridge {
        builder = builder.plugin(agent_bridge::plugin(mode));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(Mutex::new(session_link::LinkState::default()))
        .manage(Mutex::new(commands::WatcherState::default()))
        .manage(Mutex::new(terminal::TerminalState::default()))
        .manage(shared::Shared::new())
        .manage(dev_reload::DevReloadState::default())
        .manage(update_check::UpdateState::default())
        .setup(move |app| {
            if bridge {
                agent_bridge::start(app, mode);
            }
            // Links arrive through the plugin; one that launched the app is also readable once here.
            let link_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    session_link::handle_url(&link_handle, url.as_str());
                }
            });
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    session_link::handle_url(app.handle(), url.as_str());
                }
            }
            // Unbundled dev runs would otherwise show the binary name in the
            // Dock; no-op in release bundles and off macOS.
            if let Some(name) = app.config().product_name.as_deref() {
                dock_name::set_dev_dock_name(name);
            }
            dev_reload::restore_windows(app.handle());
            update_check::start(app.handle());
            precise_status::start(app.handle());
            // One app-wide poller: every 3s it refreshes the shared activity scan
            // and (only when the project dir's signature changes) re-discovers,
            // then broadcasts to ALL windows. Replaces each window polling on its
            // own, so N windows cost the same as one. Idle when no windows exist.
            let handle = app.handle().clone();
            // Last published status, so the file is rewritten only on change.
            let mut last_status: Option<status_file::StatusFile> = None;
            // Claude's own hook events, tailed and folded across ticks.
            let mut hook_tail = hooks::EventTail::new();
            let mut hook_state: std::collections::HashMap<String, hooks::HookPhase> =
                Default::default();
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

                hooks::fold_into(&mut hook_state, &hook_tail.read_new(&hooks::events_path()));
                let now = terminal::now_secs();
                // A week-old phase is a session that ended without saying so.
                hook_state.retain(|_, h| now - h.since < 7 * 86_400);

                if !hosted_now.is_empty() || !hook_state.is_empty() {
                    activity::stamp_hosted(
                        &mut outcome.activity,
                        &hosted_now,
                        &hook_state,
                        || handle.state::<shared::Shared>().sessions(&root).unwrap_or_default(),
                        &root,
                        now,
                    );
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
            } else if event.id().as_ref() == "quit" {
                close_guard::request_quit(app);
            } else if event.id().as_ref() == dev_reload::MENU_ID {
                dev_reload::start_rebuild(app);
            }
        })
        .on_window_event(|win, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                close_guard::on_close_requested(win, api);
            }
            if let WindowEvent::Focused(true) = event {
                session_link::focused(win.app_handle(), win.label());
            }
            // When a window is destroyed, drop its live-tail watcher (the tail
            // thread exits when the channel sender is dropped). A backstop for
            // hard closes where the frontend's stop_watching may not run.
            if let WindowEvent::Destroyed = event {
                session_link::forget_window(win.app_handle(), win.label());
                if let Some(state) = win.try_state::<Mutex<commands::WatcherState>>() {
                    if let Ok(mut guard) = state.lock() {
                        guard.forget_window(win.label());
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
            commands::tool_names,
            commands::refresh_sessions,
            commands::resolve_branch_messages,
            commands::open_session,
            commands::stop_watching,
            commands::search_messages,
            precise_status::precise_status_choice,
            precise_status::precise_status_enabled,
            precise_status::precise_status_enable,
            precise_status::precise_status_disable,
            commands::fork_session,
            commands::message_previews,
            commands::delete_session,
            commands::new_session,
            commands::path_exists,
            commands::read_image_file,
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
            close_guard::confirm_close,
            close_guard::confirm_quit,
            session_link::link_listener_ready,
            update_check::update_available,
            dev_reload::dev_rebuild,
            dev_reload::dev_take_view,
            dev_reload::dev_set_view,
        ])
        .build(context)
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
                // Tauri would exit 0 after this callback; scripts/dev.sh needs
                // the relaunch code, so run Tauri's cleanup and exit with it here.
                if app_handle.state::<dev_reload::DevReloadState>().relaunch_requested() {
                    app_handle.cleanup_before_exit();
                    std::process::exit(dev_reload::RELAUNCH_EXIT_CODE);
                }
            }
        });
}
