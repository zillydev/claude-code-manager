//! The macOS window id of one of our windows.
//!
//! Window managers (yabai) and status bars address windows by CGWindowID,
//! which for an AppKit window is its `windowNumber`. Publishing that id lets a
//! consumer place a session on the space its window currently occupies —
//! something ccm cannot derive from a `.jsonl` file, since nothing in a session
//! file says which window is displaying it.

#[cfg(target_os = "macos")]
pub fn window_id(app: &tauri::AppHandle, label: &str) -> Option<i64> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use tauri::Manager;

    let win = app.get_webview_window(label)?;
    let ns_window = win.ns_window().ok()? as *mut AnyObject;
    if ns_window.is_null() {
        return None;
    }
    // SAFETY: `ns_window()` hands back a live NSWindow*, and `windowNumber` is
    // a documented AppKit property returning NSInteger.
    let number: isize = unsafe { msg_send![ns_window, windowNumber] };
    Some(number as i64)
}

#[cfg(not(target_os = "macos"))]
pub fn window_id(_app: &tauri::AppHandle, _label: &str) -> Option<i64> {
    None
}
