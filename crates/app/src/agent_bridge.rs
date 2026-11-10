//! Debug-only bridge for a coding agent to see and drive the app via `scripts/ui`: a hidden,
//! never-activating copy (`CM_AGENT=1`) or a normal visible window (`CM_GUIDED=1`). One JSON
//! line per request and reply over `agent.sock` in the config dir:
//!
//! - `{"cmd":"windows"}`
//! - `{"cmd":"snap","window":"main","path":"/tmp/x.png","width":800}` (width in points, optional)
//! - `{"cmd":"eval","window":"main","js":"<async function body>"}`
//! - `{"cmd":"click","window":"main","x":10,"y":20,"count":1}` (CSS px from the top left)
//! - `{"cmd":"hover","window":"main","x":10,"y":20}`
//! - `{"cmd":"key","window":"main","key":"Enter","mods":["cmd"]}`
//! - `{"cmd":"type","window":"main","text":"hello"}`
//! - `{"cmd":"resize","window":"main","width":1200,"height":800}`
//! - `{"cmd":"native","window":"main"}` (AppKit visibility state, for diagnosing a hidden page)

use std::ffi::OsString;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Off,
    /// The hidden ghost copy, whose `~/.claude` is the user's real one.
    Agent,
    /// A visible window on a sandboxed `~/.claude`.
    Guided,
}

pub fn mode() -> Mode {
    if !cfg!(all(debug_assertions, target_os = "macos")) {
        return Mode::Off;
    }
    mode_from(std::env::var_os("CM_AGENT"), std::env::var_os("CM_GUIDED"))
}

fn mode_from(agent: Option<OsString>, guided: Option<OsString>) -> Mode {
    match (agent, guided) {
        (Some(_), _) => Mode::Agent,
        (None, Some(_)) => Mode::Guided,
        (None, None) => Mode::Off,
    }
}

/// The agent title lets window managers (yabai) be told to leave its windows alone.
pub fn window_title(mode: Mode) -> &'static str {
    match mode {
        Mode::Agent => "CCM Agent",
        Mode::Guided => "Claude Code Manager — guided",
        Mode::Off => "Claude Code Manager",
    }
}

pub fn enabled() -> bool {
    mode() == Mode::Agent
}

/// For commands that write the user's real `~/.claude`: the agent copy's config dir is
/// throwaway, but its `~/.claude` is not.
pub fn refuse_in_agent(what: &str) -> Result<(), String> {
    refusal(enabled(), what)
}

fn refusal(agent: bool, what: &str) -> Result<(), String> {
    if agent {
        Err(format!("The agent copy of the app can't {what}: it would change the real ~/.claude."))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{mode_from, refusal, window_title, Mode};

    fn set() -> Option<OsString> {
        Some("1".into())
    }

    #[test]
    fn agent_wins_over_guided() {
        assert_eq!(mode_from(None, None), Mode::Off);
        assert_eq!(mode_from(set(), None), Mode::Agent);
        assert_eq!(mode_from(None, set()), Mode::Guided);
        assert_eq!(mode_from(set(), set()), Mode::Agent);
    }

    #[test]
    fn guided_window_is_titled_for_people() {
        assert_eq!(window_title(Mode::Guided), "Claude Code Manager — guided");
    }

    #[test]
    fn refuses_only_in_the_agent_copy() {
        assert!(refusal(false, "delete a session").is_ok());
        let err = refusal(true, "delete a session").unwrap_err();
        assert!(err.contains("can't delete a session"), "{err}");
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
pub use imp::*;

#[cfg(not(all(debug_assertions, target_os = "macos")))]
mod stub {
    use super::Mode;

    pub fn prepare(_: &mut tauri::Context<tauri::Wry>, _: Mode) {}
    pub fn plugin(_: Mode) -> tauri::plugin::TauriPlugin<tauri::Wry> {
        tauri::plugin::Builder::new("agent-bridge").build()
    }
    pub fn start(_: &mut tauri::App, _: Mode) {}
}
#[cfg(not(all(debug_assertions, target_os = "macos")))]
pub use stub::*;

#[cfg(all(debug_assertions, target_os = "macos"))]
mod imp {
    use std::ffi::{c_void, CStr};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::encode::{Encode, Encoding};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};
    use serde_json::{json, Value};
    use tauri::{AppHandle, Manager, WebviewWindow};

    use super::{window_title, Mode};

    /// Console capture: `window.__cmAgentLog` holds the page's console errors and warnings,
    /// uncaught errors and rejections, for `scripts/ui logs`.
    const INIT_SCRIPT: &str = r#"(() => {
      if (window.__cmAgentLog) return;
      const log = (window.__cmAgentLog = []);
      const push = (level, args) => {
        log.push({ level, at: Date.now(), text: args.map((a) => {
          if (a instanceof Error) return a.stack || String(a);
          if (typeof a === "string") return a;
          try { return JSON.stringify(a); } catch { return String(a); }
        }).join(" ") });
        if (log.length > 500) log.shift();
      };
      for (const level of ["error", "warn"]) {
        const orig = console[level].bind(console);
        console[level] = (...args) => { push(level, args); orig(...args); };
      }
      window.addEventListener("error", (e) => push("uncaught", [e.error || e.message]));
      window.addEventListener("unhandledrejection", (e) => push("unhandled", [e.reason]));
      window.addEventListener("keydown", (e) => { window.__cmAgentLastKey = e; }, true);
    })();"#;

    const TIMEOUT: Duration = Duration::from_secs(15);

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    unsafe impl Encode for Point {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Insets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }
    unsafe impl Encode for Insets {
        const ENCODING: Encoding =
            Encoding::Struct("NSEdgeInsets", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }

    /// Every window is titled for its mode and loads that copy's own Vite server
    /// (`CM_DEV_URL`) so the binary stays the one `make dev` builds; agent windows start hidden.
    pub fn prepare(context: &mut tauri::Context<tauri::Wry>, mode: Mode) {
        let config = context.config_mut();
        for w in config.app.windows.iter_mut() {
            if mode == Mode::Agent {
                w.visible = false;
                w.focus = false;
            }
            w.title = window_title(mode).into();
        }
        if let Some(url) = std::env::var("CM_DEV_URL").ok().and_then(|u| u.parse().ok()) {
            config.build.dev_url = Some(url);
        }
    }

    /// In agent mode, ghosts each window as its page finishes loading: ordered in any
    /// earlier, during launch, AppKit takes the order back.
    pub fn plugin(mode: Mode) -> tauri::plugin::TauriPlugin<tauri::Wry> {
        let builder = tauri::plugin::Builder::new("agent-bridge").js_init_script(INIT_SCRIPT);
        if mode != Mode::Agent {
            return builder.build();
        }
        builder
            .on_page_load(|webview, payload| {
                if payload.event() == tauri::webview::PageLoadEvent::Finished {
                    if let Some(w) = webview.app_handle().get_webview_window(webview.label()) {
                        ghost(&w);
                    }
                }
            })
            .build()
    }

    /// Runs in `setup`, before tao's launch-time activate, which Prohibited then ignores.
    pub fn start(app: &mut tauri::App, mode: Mode) {
        if mode == Mode::Agent {
            app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
        }
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("agent bridge: cannot bind {}: {e}", path.display());
                return;
            }
        };
        eprintln!("agent bridge: listening on {}", path.display());
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handle = handle.clone();
                std::thread::spawn(move || serve(&handle, stream));
            }
        });
    }

    fn socket_path() -> PathBuf {
        crate::config::config_dir().join("agent.sock")
    }

    /// Transparent, mouse-blind, behind everything, on every Space (a Space switch would
    /// otherwise occlude it and stop rAF), and out of the ⌘` cycle.
    fn ghost(win: &WebviewWindow) {
        let _ = win.set_title(window_title(Mode::Agent));
        let _ = win.with_webview(|pw| unsafe {
            let ns_window = pw.ns_window() as *mut AnyObject;
            let wv = pw.inner() as *mut AnyObject;
            if ns_window.is_null() {
                return;
            }
            let _: () = msg_send![ns_window, setAlphaValue: 0.0f64];
            let _: () = msg_send![ns_window, setIgnoresMouseEvents: Bool::YES];
            let _: () = msg_send![ns_window, setHasShadow: Bool::NO];
            // CanJoinAllSpaces | IgnoresCycle
            let _: () = msg_send![ns_window, setCollectionBehavior: (1usize << 0) | (1usize << 6)];
            // WebKit reads the page as visible only once the window has been ordered front.
            let _: () = msg_send![ns_window, orderFrontRegardless];
            let _: () = msg_send![ns_window, orderBack: std::ptr::null::<AnyObject>()];
            if !wv.is_null() {
                let _: Bool = msg_send![ns_window, makeFirstResponder: wv];
                // Private WebKit switch; keeps the page "visible" even when covered.
                let s: Sel = sel!(_setWindowOcclusionDetectionEnabled:);
                let responds: Bool = msg_send![wv, respondsToSelector: s];
                if responds.as_bool() {
                    let _: () = msg_send![wv, _setWindowOcclusionDetectionEnabled: Bool::NO];
                }
            }
        });
    }

    fn serve(app: &AppHandle, stream: UnixStream) {
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => return,
        };
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => handle(app, &req).unwrap_or_else(|e| json!({ "ok": false, "error": e })),
                Err(e) => json!({ "ok": false, "error": format!("bad request: {e}") }),
            };
            if writeln!(writer, "{reply}").is_err() {
                return;
            }
        }
    }

    fn handle(app: &AppHandle, req: &Value) -> Result<Value, String> {
        let cmd = req["cmd"].as_str().ok_or("missing cmd")?;
        if cmd == "windows" {
            return Ok(windows(app));
        }
        let label = req["window"].as_str().unwrap_or("main");
        let win = app.get_webview_window(label).ok_or_else(|| format!("no window {label:?}"))?;
        let num = |k: &str| req[k].as_f64().ok_or_else(|| format!("missing {k}"));
        match cmd {
            "snap" => {
                let path = req["path"].as_str().ok_or("missing path")?.to_string();
                snap(&win, path, req["width"].as_f64())
            }
            "eval" => eval(&win, req["js"].as_str().ok_or("missing js")?.to_string()),
            "click" => {
                let count = req["count"].as_u64().unwrap_or(1).max(1) as isize;
                mouse(&win, num("x")?, num("y")?, Some(count))
            }
            "hover" => mouse(&win, num("x")?, num("y")?, None),
            "key" => {
                let key = req["key"].as_str().ok_or("missing key")?;
                let mods: Vec<&str> =
                    req["mods"].as_array().map(|a| a.iter().filter_map(|m| m.as_str()).collect()).unwrap_or_default();
                let (chars, code) = key_spec(key)?;
                let flags = modifier_flags(&mods)?;
                if flags & (CMD | CTRL) == 0 {
                    return keys(&win, vec![(chars, code)], flags);
                }
                eval(&win, "window.__cmAgentLastKey = null".into())?;
                keys(&win, vec![(chars.clone(), code)], flags)?;
                let prevented = eval(&win, "return !!window.__cmAgentLastKey?.defaultPrevented".into())?;
                if prevented["value"].as_bool() == Some(true) {
                    return Ok(json!({ "ok": true }));
                }
                menu_equivalent(&win, chars, flags)
            }
            "type" => {
                let text = req["text"].as_str().ok_or("missing text")?;
                let strokes = text
                    .chars()
                    .map(|c| match c {
                        '\n' => key_spec("Enter").unwrap(),
                        '\t' => key_spec("Tab").unwrap(),
                        c => (c.to_string(), 0),
                    })
                    .collect();
                keys(&win, strokes, 0)
            }
            "native" => native(&win),
            "resize" => {
                win.set_size(tauri::LogicalSize::new(num("width")?, num("height")?)).map_err(|e| e.to_string())?;
                Ok(json!({ "ok": true }))
            }
            other => Err(format!("unknown cmd {other:?}")),
        }
    }

    fn windows(app: &AppHandle) -> Value {
        let list: Vec<Value> = app
            .webview_windows()
            .iter()
            .map(|(label, w)| {
                let scale = w.scale_factor().unwrap_or(1.0);
                let size = w.inner_size().map(|s| s.to_logical::<f64>(scale)).ok();
                json!({
                    "label": label,
                    "url": w.url().map(|u| u.to_string()).ok(),
                    "width": size.map(|s| s.width),
                    "height": size.map(|s| s.height),
                    "scale": scale,
                })
            })
            .collect();
        json!({ "ok": true, "windows": list })
    }

    /// Runs `f` on the main thread with the window's NSWindow and WKWebView; `f` replies
    /// through the sender, possibly later from a WebKit completion handler.
    fn on_main(
        win: &WebviewWindow,
        f: impl FnOnce(*mut AnyObject, *mut AnyObject, mpsc::Sender<Result<Value, String>>) + Send + 'static,
    ) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        win.with_webview(move |pw| f(pw.ns_window() as *mut AnyObject, pw.inner() as *mut AnyObject, tx))
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(TIMEOUT).map_err(|_| "timed out waiting for the webview".to_string())?
    }

    fn snap(win: &WebviewWindow, path: String, width: Option<f64>) -> Result<Value, String> {
        on_main(win, move |_, wv, tx| unsafe {
            let config: Option<Retained<AnyObject>> = match AnyClass::get(c"WKSnapshotConfiguration") {
                Some(cls) => msg_send![msg_send![cls, alloc], init],
                None => None,
            };
            if let Some(c) = &config {
                // The default rect is the bounds, which run under the title bar, measured
                // from the page's origin: that leaves a blank strip the inset's height.
                let bounds: Rect = msg_send![wv, bounds];
                let insets: Insets = msg_send![wv, safeAreaInsets];
                let rect = Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size { width: bounds.size.width, height: bounds.size.height - insets.top },
                };
                let _: () = msg_send![&**c, setRect: rect];
                if let Some(w) = width {
                    let n: Retained<AnyObject> = msg_send![AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: w];
                    let _: () = msg_send![&**c, setSnapshotWidth: &*n];
                }
            }
            let block = RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
                let _ = tx.send(if image.is_null() {
                    Err(format!("snapshot failed: {}", describe(error)))
                } else {
                    write_png(image, &path)
                });
            });
            let config_ptr = config.as_deref().map_or(std::ptr::null(), |c| c as *const AnyObject);
            let _: () = msg_send![wv, takeSnapshotWithConfiguration: config_ptr, completionHandler: &*block];
        })
    }

    unsafe fn write_png(image: *mut AnyObject, path: &str) -> Result<Value, String> {
        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        let rep: *mut AnyObject = msg_send![AnyClass::get(c"NSBitmapImageRep").unwrap(), imageRepWithData: tiff];
        if rep.is_null() {
            return Err("could not read the snapshot".into());
        }
        let props: *mut AnyObject = msg_send![AnyClass::get(c"NSDictionary").unwrap(), dictionary];
        // NSBitmapImageFileTypePNG
        let png: *mut AnyObject = msg_send![rep, representationUsingType: 4usize, properties: props];
        if png.is_null() {
            return Err("could not encode PNG".into());
        }
        let bytes: *const c_void = msg_send![png, bytes];
        let len: usize = msg_send![png, length];
        let data = std::slice::from_raw_parts(bytes as *const u8, len);
        std::fs::write(path, data).map_err(|e| format!("write {path}: {e}"))?;
        let w: isize = msg_send![rep, pixelsWide];
        let h: isize = msg_send![rep, pixelsHigh];
        Ok(json!({ "ok": true, "path": path, "width": w, "height": h }))
    }

    /// `js` is an async function body; its return value comes back JSON-encoded, and a
    /// throw comes back as the error.
    fn eval(win: &WebviewWindow, js: String) -> Result<Value, String> {
        let wrapped = format!(
            "try {{ const __v = await (async () => {{ {js}\n}})(); \
             return JSON.stringify({{ ok: true, value: __v === undefined ? null : __v }}); }} \
             catch (e) {{ return JSON.stringify({{ ok: false, error: String((e && e.stack) || e) }}); }}"
        );
        on_main(win, move |_, wv, tx| unsafe {
            let source = nsstring(&wrapped);
            let args: *mut AnyObject = msg_send![AnyClass::get(c"NSDictionary").unwrap(), dictionary];
            let world: *mut AnyObject = msg_send![AnyClass::get(c"WKContentWorld").unwrap(), pageWorld];
            let block = RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
                let reply = if result.is_null() {
                    Err(format!("eval failed: {}", describe(error)))
                } else {
                    let text = rust_string(result);
                    serde_json::from_str::<Value>(&text).map_err(|e| format!("bad eval result: {e}")).and_then(|v| {
                        if v["ok"].as_bool() == Some(true) {
                            Ok(v)
                        } else {
                            Err(v["error"].as_str().unwrap_or("eval threw").to_string())
                        }
                    })
                };
                let _ = tx.send(reply);
            });
            let _: () = msg_send![
                wv,
                callAsyncJavaScript: &*source,
                arguments: args,
                inFrame: std::ptr::null::<AnyObject>(),
                inContentWorld: world,
                completionHandler: &*block
            ];
        })
    }

    /// The window's AppKit state, for diagnosing a page that reads hidden.
    fn native(win: &WebviewWindow) -> Result<Value, String> {
        on_main(win, move |ns_window, wv, tx| unsafe {
            let visible: Bool = msg_send![ns_window, isVisible];
            let occlusion: usize = msg_send![ns_window, occlusionState];
            let alpha: f64 = msg_send![ns_window, alphaValue];
            let frame: Rect = msg_send![ns_window, frame];
            let on_space: Bool = msg_send![ns_window, isOnActiveSpace];
            let s: Sel = sel!(_setWindowOcclusionDetectionEnabled:);
            let private_switch: Bool = msg_send![wv, respondsToSelector: s];
            let insets: Insets = msg_send![wv, safeAreaInsets];
            let bounds: Rect = msg_send![wv, bounds];
            let flipped: Bool = msg_send![wv, isFlipped];
            let _ = tx.send(Ok(json!({
                "ok": true,
                "isVisible": visible.as_bool(),
                // NSWindowOcclusionStateVisible = 1 << 1
                "occlusionVisible": occlusion & 2 != 0,
                "alpha": alpha,
                "frame": [frame.origin.x, frame.origin.y, frame.size.width, frame.size.height],
                "onActiveSpace": on_space.as_bool(),
                "occlusionSwitch": private_switch.as_bool(),
                "safeArea": [insets.top, insets.left, insets.bottom, insets.right],
                "bounds": [bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height],
                "flipped": flipped.as_bool(),
            })));
        })
    }

    /// CSS px → the event's point in window coordinates. The page starts below the
    /// title bar, which the webview reports as its top safe-area inset.
    unsafe fn window_point(wv: *mut AnyObject, x: f64, y: f64) -> Point {
        let bounds: Rect = msg_send![wv, bounds];
        let flipped: Bool = msg_send![wv, isFlipped];
        let insets: Insets = msg_send![wv, safeAreaInsets];
        let (x, y) = (x + insets.left, y + insets.top);
        let local = Point { x, y: if flipped.as_bool() { y } else { bounds.size.height - y } };
        msg_send![wv, convertPoint: local, toView: std::ptr::null::<AnyObject>()]
    }

    unsafe fn uptime() -> f64 {
        let info: *mut AnyObject = msg_send![AnyClass::get(c"NSProcessInfo").unwrap(), processInfo];
        msg_send![info, systemUptime]
    }

    /// `count` None moves the pointer only (hover); Some(n) clicks with clickCount n.
    fn mouse(win: &WebviewWindow, x: f64, y: f64, count: Option<isize>) -> Result<Value, String> {
        on_main(win, move |ns_window, wv, tx| unsafe {
            let at = window_point(wv, x, y);
            let number: isize = msg_send![ns_window, windowNumber];
            let event = |kind: usize, clicks: isize| -> *mut AnyObject {
                msg_send![
                    AnyClass::get(c"NSEvent").unwrap(),
                    mouseEventWithType: kind,
                    location: at,
                    modifierFlags: 0usize,
                    timestamp: uptime(),
                    windowNumber: number,
                    context: std::ptr::null::<AnyObject>(),
                    eventNumber: 0isize,
                    clickCount: clicks,
                    pressure: 1.0f32
                ]
            };
            // NSEventTypeMouseMoved, LeftMouseDown, LeftMouseUp
            let _: () = msg_send![wv, mouseMoved: event(5, 0)];
            if let Some(n) = count {
                for i in 1..=n {
                    let _: () = msg_send![wv, mouseDown: event(1, i)];
                    let _: () = msg_send![wv, mouseUp: event(2, i)];
                }
            }
            reply_when_drained(wv, sel!(_doAfterProcessingAllPendingMouseEvents:), tx);
        })
    }

    /// Replies once the web process has handled the queued input, so the caller's next
    /// eval can't overtake it; WebKit sends input one event at a time, eval at once.
    unsafe fn reply_when_drained(wv: *mut AnyObject, drained: Sel, tx: mpsc::Sender<Result<Value, String>>) {
        let responds: Bool = msg_send![wv, respondsToSelector: drained];
        if !responds.as_bool() {
            let _ = tx.send(Ok(json!({ "ok": true })));
            return;
        }
        let block = RcBlock::new(move || {
            let _ = tx.send(Ok(json!({ "ok": true })));
        });
        let _: () = msg_send![wv, performSelector: drained, withObject: &*block as *const _ as *const AnyObject];
    }

    fn keys(win: &WebviewWindow, strokes: Vec<(String, u16)>, flags: usize) -> Result<Value, String> {
        on_main(win, move |ns_window, wv, tx| unsafe {
            let number: isize = msg_send![ns_window, windowNumber];
            for (chars, code) in &strokes {
                let s = nsstring(chars);
                let event = |kind: usize| -> *mut AnyObject {
                    msg_send![
                        AnyClass::get(c"NSEvent").unwrap(),
                        keyEventWithType: kind,
                        location: Point { x: 0.0, y: 0.0 },
                        modifierFlags: flags,
                        timestamp: uptime(),
                        windowNumber: number,
                        context: std::ptr::null::<AnyObject>(),
                        characters: &*s,
                        charactersIgnoringModifiers: &*s,
                        isARepeat: Bool::NO,
                        keyCode: *code
                    ]
                };
                // NSEventTypeKeyDown, KeyUp
                let _: () = msg_send![wv, keyDown: event(10)];
                let _: () = msg_send![wv, keyUp: event(11)];
            }
            reply_when_drained(wv, sel!(_doAfterProcessingAllPendingKeyEvents:), tx);
        })
    }

    /// A key name → (characters, macOS virtual key code). A single character types itself.
    fn key_spec(key: &str) -> Result<(String, u16), String> {
        let fkey = |c: u32| char::from_u32(c).unwrap().to_string();
        Ok(match key {
            "Enter" | "Return" => ("\r".into(), 36),
            "Tab" => ("\t".into(), 48),
            "Space" => (" ".into(), 49),
            "Backspace" => ("\u{7f}".into(), 51),
            "Escape" => ("\u{1b}".into(), 53),
            "Delete" => (fkey(0xF728), 117),
            "ArrowLeft" => (fkey(0xF702), 123),
            "ArrowRight" => (fkey(0xF703), 124),
            "ArrowDown" => (fkey(0xF701), 125),
            "ArrowUp" => (fkey(0xF700), 126),
            "Home" => (fkey(0xF729), 115),
            "End" => (fkey(0xF72B), 119),
            "PageUp" => (fkey(0xF72C), 116),
            "PageDown" => (fkey(0xF72D), 121),
            k if k.chars().count() == 1 => (k.to_lowercase(), letter_code(k.chars().next().unwrap())),
            other => return Err(format!("unknown key {other:?}")),
        })
    }

    /// ANSI virtual key codes for letters and digits, so `event.code` reads right for shortcuts.
    fn letter_code(c: char) -> u16 {
        const LETTERS: [u16; 26] =
            [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6];
        const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
        match c.to_ascii_lowercase() {
            l @ 'a'..='z' => LETTERS[(l as u8 - b'a') as usize],
            d @ '0'..='9' => DIGITS[(d as u8 - b'0') as usize],
            ',' => 43,
            '.' => 47,
            '/' => 44,
            _ => 0,
        }
    }

    const SHIFT: usize = 1 << 17;
    const CTRL: usize = 1 << 18;
    const ALT: usize = 1 << 19;
    const CMD: usize = 1 << 20;

    fn modifier_flags(mods: &[&str]) -> Result<usize, String> {
        mods.iter().try_fold(0usize, |acc, m| {
            Ok(acc
                | match *m {
                    "shift" => SHIFT,
                    "ctrl" | "control" => CTRL,
                    "alt" | "opt" | "option" => ALT,
                    "cmd" | "meta" | "command" => CMD,
                    other => return Err(format!("unknown modifier {other:?}")),
                })
        })
    }

    /// Runs the menu item bound to a shortcut the page didn't consume, as AppKit does; a nil
    /// target means the webview, since a Prohibited app never has the key window AppKit uses.
    fn menu_equivalent(win: &WebviewWindow, chars: String, flags: usize) -> Result<Value, String> {
        on_main(win, move |_, wv, tx| unsafe {
            let app: *mut AnyObject = msg_send![AnyClass::get(c"NSApplication").unwrap(), sharedApplication];
            let menu: *mut AnyObject = msg_send![app, mainMenu];
            let Some(item) = find_menu_item(menu, &chars, flags) else {
                let _ = tx.send(Ok(json!({ "ok": true, "menu": null })));
                return;
            };
            let title = rust_string(msg_send![item, title]);
            let action: Option<Sel> = msg_send![item, action];
            let target: *mut AnyObject = msg_send![item, target];
            if let Some(action) = action {
                let _: Bool = if target.is_null() {
                    msg_send![wv, tryToPerform: action, with: item]
                } else {
                    msg_send![app, sendAction: action, to: target, from: item]
                };
            }
            let _ = tx.send(Ok(json!({ "ok": true, "menu": title })));
        })
    }

    unsafe fn find_menu_item(menu: *mut AnyObject, chars: &str, flags: usize) -> Option<*mut AnyObject> {
        if menu.is_null() {
            return None;
        }
        let count: isize = msg_send![menu, numberOfItems];
        for i in 0..count {
            let item: *mut AnyObject = msg_send![menu, itemAtIndex: i];
            let submenu: *mut AnyObject = msg_send![item, submenu];
            if let Some(found) = find_menu_item(submenu, chars, flags) {
                return Some(found);
            }
            let key = rust_string(msg_send![item, keyEquivalent]);
            if key.is_empty() {
                continue;
            }
            let mut mask: usize = msg_send![item, keyEquivalentModifierMask];
            // An uppercase key equivalent ("Z" for Redo) implies Shift.
            if key.chars().any(|c| c.is_uppercase()) {
                mask |= SHIFT;
            }
            if key.to_lowercase() == chars && mask & (SHIFT | CTRL | ALT | CMD) == flags {
                return Some(item);
            }
        }
        None
    }

    unsafe fn nsstring(s: &str) -> Retained<AnyObject> {
        let cls = AnyClass::get(c"NSString").unwrap();
        let obj: Allocated<AnyObject> = msg_send![cls, alloc];
        // NSUTF8StringEncoding
        msg_send![obj, initWithBytes: s.as_ptr() as *const c_void, length: s.len(), encoding: 4usize]
    }

    unsafe fn rust_string(ns: *mut AnyObject) -> String {
        let utf8: *const std::ffi::c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }

    unsafe fn describe(error: *mut AnyObject) -> String {
        if error.is_null() {
            return "unknown error".into();
        }
        let desc: *mut AnyObject = msg_send![error, localizedDescription];
        rust_string(desc)
    }
}
