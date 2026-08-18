//! Dev-only macOS nicety: label the Dock tile with the product name.
//!
//! `cargo tauri dev` runs the bare `target/debug/<bin>` executable — no
//! `.app` wrapper — and LaunchServices names such a process after its
//! *file*, so the Dock, ⌘-Tab and Force Quit show `claude-code-manager-app`.
//! Tauri does embed an Info.plist (with `CFBundleName`) into dev binaries,
//! but LS never consults it for unbundled executables: `lsappinfo info -pid
//! <pid>` reports `LSDisplayName = <filename>`, `bundleID = NULL`. The
//! bundled release app takes its name from the real Info.plist and doesn't
//! need any of this, hence the `debug_assertions` gate.
//!
//! Renaming the binary isn't an option either (rustc rejects spaces in crate
//! names, and `mainBinaryName` only applies at `tauri build`), so this does
//! what the JDK (`-Xdock:name`), Chromium (`base::mac::SetProcessName`) and
//! WebKit do for the same problem: tell LS the display name of the current
//! process through its private `_LSSetApplicationInformationItem`. The
//! symbols are resolved with `dlsym` at runtime — nothing links against
//! private API — and every lookup failure is a silent no-op, so a future
//! macOS that drops them just falls back to the filename.
//!
//! Call it before the app finishes launching (Tauri's `setup` runs before the
//! event loop, i.e. before `applicationDidFinishLaunching`) so the Dock tile
//! is created with the right name rather than renamed after the fact.

#[cfg(all(target_os = "macos", debug_assertions))]
pub fn set_dev_dock_name(name: &str) {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::ffi::{c_void, CStr};

    type GetCurrentApplicationAsn = unsafe extern "C" fn() -> *const c_void;
    type SetApplicationInformationItem = unsafe extern "C" fn(
        session: i32,
        asn: *const c_void,
        key: CFStringRef,
        value: CFStringRef,
        out: *mut c_void,
    ) -> i32;

    // SAFETY: dlsym with a valid NUL-terminated name; RTLD_DEFAULT searches
    // every image already loaded (AppKit pulls LaunchServices in).
    unsafe fn sym(name: &CStr) -> *mut c_void {
        libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr())
    }

    // SAFETY: the function signatures mirror the ones the JDK/Chromium use;
    // pointers are null-checked before being called or dereferenced, and the
    // CFString outlives the call.
    unsafe {
        let get_asn = sym(c"_LSGetCurrentApplicationASN");
        let set_item = sym(c"_LSSetApplicationInformationItem");
        let key = sym(c"_kLSDisplayNameKey") as *const CFStringRef;
        if get_asn.is_null() || set_item.is_null() || key.is_null() {
            return;
        }
        let get_asn: GetCurrentApplicationAsn = std::mem::transmute(get_asn);
        let set_item: SetApplicationInformationItem = std::mem::transmute(set_item);
        let asn = get_asn();
        if asn.is_null() {
            return;
        }
        let value = CFString::new(name);
        // -2 is the "current session" constant WebKit/Chromium/JDK all pass.
        let _ = set_item(
            -2,
            asn,
            *key,
            value.as_concrete_TypeRef(),
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(all(target_os = "macos", debug_assertions)))]
pub fn set_dev_dock_name(_name: &str) {}
