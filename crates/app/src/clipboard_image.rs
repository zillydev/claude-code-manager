//! "Copy as image": puts a PNG the frontend rendered from a message onto the pasteboard.

use std::path::{Path, PathBuf};

use tauri::ipc::{InvokeBody, Request};

use crate::{agent_bridge, config};

/// Takes the PNG as a raw IPC body. Returns the file written instead, in the agent copy.
#[tauri::command]
pub fn copy_image_png(request: Request<'_>) -> Result<Option<String>, String> {
    let InvokeBody::Raw(png) = request.body() else {
        return Err("expected raw PNG bytes".into());
    };
    check_png(png)?;
    if let Some(path) = agent_target(agent_bridge::enabled(), &config::config_dir()) {
        std::fs::write(&path, png).map_err(|e| format!("write {}: {e}", path.display()))?;
        return Ok(Some(path.display().to_string()));
    }
    write_pasteboard(png)?;
    Ok(None)
}

fn check_png(bytes: &[u8]) -> Result<(), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(())
    } else {
        Err("not a PNG".into())
    }
}

/// The hidden agent copy must not clobber the user's real clipboard; it writes a file to check instead.
fn agent_target(agent: bool, config_dir: &Path) -> Option<PathBuf> {
    agent.then(|| config_dir.join("agent-clipboard.png"))
}

#[cfg(target_os = "macos")]
fn write_pasteboard(png: &[u8]) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let cls = AnyClass::get(c"NSPasteboard").ok_or("NSPasteboard missing")?;
        let board: Option<Retained<AnyObject>> = msg_send![cls, generalPasteboard];
        mac::write_to(&*board.ok_or("no general pasteboard")?, png)
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_void, CStr};

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};

    fn class(name: &CStr) -> Result<&'static AnyClass, String> {
        AnyClass::get(name).ok_or_else(|| format!("{name:?} missing"))
    }

    pub(super) unsafe fn ns_string(s: &CStr) -> Result<Retained<AnyObject>, String> {
        let r: Option<Retained<AnyObject>> = msg_send![class(c"NSString")?, stringWithUTF8String: s.as_ptr()];
        r.ok_or_else(|| "NSString failed".into())
    }

    /// PNG plus a TIFF flavour: some apps only read TIFF from the pasteboard.
    pub(super) unsafe fn write_to(board: &AnyObject, png: &[u8]) -> Result<(), String> {
        let data: Option<Retained<AnyObject>> =
            msg_send![class(c"NSData")?, dataWithBytes: png.as_ptr().cast::<c_void>(), length: png.len()];
        let data = data.ok_or("could not wrap the PNG")?;
        let image: Option<Retained<AnyObject>> = msg_send![msg_send![class(c"NSImage")?, alloc], initWithData: &*data];
        let image = image.ok_or("could not decode the PNG")?;
        let tiff: Option<Retained<AnyObject>> = msg_send![&*image, TIFFRepresentation];
        let item: Option<Retained<AnyObject>> = msg_send![msg_send![class(c"NSPasteboardItem")?, alloc], init];
        let item = item.ok_or("could not create a pasteboard item")?;
        let _: bool = msg_send![&*item, setData: &*data, forType: &*ns_string(c"public.png")?];
        if let Some(tiff) = tiff {
            let _: bool = msg_send![&*item, setData: &*tiff, forType: &*ns_string(c"public.tiff")?];
        }
        let items: Option<Retained<AnyObject>> = msg_send![class(c"NSArray")?, arrayWithObject: &*item];
        let items = items.ok_or("could not create the item list")?;
        let _: isize = msg_send![board, clearContents];
        let ok: bool = msg_send![board, writeObjects: &*items];
        if ok {
            Ok(())
        } else {
            Err("the pasteboard refused the image".into())
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn write_pasteboard(_: &[u8]) -> Result<(), String> {
    Err("Copy as image is macOS-only".into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{agent_target, check_png};

    #[test]
    fn rejects_bodies_that_are_not_png() {
        assert!(check_png(b"").is_err());
        assert!(check_png(b"GIF89a....").is_err());
        assert!(check_png(b"\x89PNG\r\n\x1a\n rest").is_ok());
    }

    #[test]
    fn only_the_agent_copy_writes_a_file() {
        let dir = Path::new("/tmp/cfg");
        assert_eq!(agent_target(false, dir), None);
        assert_eq!(agent_target(true, dir), Some(dir.join("agent-clipboard.png")));
    }

    /// Runs the real AppKit calls against a private pasteboard, never the user's clipboard.
    #[cfg(target_os = "macos")]
    #[test]
    fn writes_png_and_tiff_flavours() {
        use base64::Engine;
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject};

        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
            .unwrap();
        unsafe {
            let cls = AnyClass::get(c"NSPasteboard").unwrap();
            let board: Retained<AnyObject> = msg_send![cls, pasteboardWithUniqueName];
            super::mac::write_to(&board, &png).unwrap();
            let len = |ty: &std::ffi::CStr| -> usize {
                let d: Option<Retained<AnyObject>> = msg_send![&*board, dataForType: &*super::mac::ns_string(ty).unwrap()];
                d.map_or(0, |d| msg_send![&*d, length])
            };
            assert_eq!(len(c"public.png"), png.len());
            assert!(len(c"public.tiff") > 0);
            let _: () = msg_send![&*board, releaseGlobally];
        }
    }
}
