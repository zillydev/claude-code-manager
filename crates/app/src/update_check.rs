//! Whether a newer release is out: one background check for the whole app, reported to every window.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use semver::Version;
use tauri::{AppHandle, Emitter, Manager};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Time between checks, measured on the wall clock so a sleeping Mac still checks when it wakes.
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

const LATEST_URL: &str =
    "https://api.github.com/repos/zillydev/claude-code-manager/releases/latest";
const FIRST_CHECK: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(10 * 60);

/// The newest release found that is newer than this build.
#[derive(Default)]
pub struct UpdateState(Mutex<Option<String>>);

fn parse_version(s: &str) -> Option<Version> {
    Version::parse(s.strip_prefix('v').unwrap_or(s)).ok()
}

/// The release `tag` names (`v0.1.3` or `0.1.3`), when it is newer than `running`.
pub fn newer_release(running: &Version, tag: &str) -> Option<Version> {
    parse_version(tag).filter(|v| v > running)
}

/// Whether a check is due: never attempted, `INTERVAL` since the last attempt, or the clock went backwards.
pub fn due(last: Option<SystemTime>, now: SystemTime) -> bool {
    match last.map(|t| now.duration_since(t)) {
        None | Some(Err(_)) => true,
        Some(Ok(elapsed)) => elapsed >= INTERVAL,
    }
}

/// `tag_name` from a GitHub release body.
pub fn tag_name(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    serde_json::from_str::<Release>(body)
        .ok()
        .map(|r| r.tag_name)
}

/// The version to check against, or `None` to not check: debug builds only check as `CM_UPDATE_CHECK_AS`.
pub fn check_as(debug: bool, pretend: Option<&str>, running: &Version) -> Option<Version> {
    if !debug {
        return Some(running.clone());
    }
    parse_version(pretend?)
}

/// The latest release's tag, or `None` on any network, HTTP or parse failure.
fn fetch_latest_tag() -> Option<String> {
    // ureq defaults to rustls and Mozilla's roots; the Keychain's roots also trust a proxy's CA.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into();
    let mut response = agent
        .get(LATEST_URL)
        .header("User-Agent", "claude-code-manager")
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?;
    tag_name(&response.body_mut().read_to_string().ok()?)
}

/// Starts the app-wide check; windows learn of a release through `update-available` or `update_available`.
pub fn start(app: &AppHandle) {
    let pretend = std::env::var("CM_UPDATE_CHECK_AS").ok();
    let Some(running) = check_as(
        cfg!(debug_assertions),
        pretend.as_deref(),
        &app.package_info().version,
    ) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        let mut last = None;
        loop {
            let now = SystemTime::now();
            if due(last, now) {
                last = Some(now);
                if let Some(release) =
                    fetch_latest_tag().and_then(|tag| newer_release(&running, &tag))
                {
                    record(&app, release.to_string());
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

fn record(app: &AppHandle, version: String) {
    let state = app.state::<UpdateState>();
    let Ok(mut known) = state.0.lock() else {
        return;
    };
    if known.as_deref() == Some(version.as_str()) {
        return;
    }
    *known = Some(version.clone());
    drop(known);
    let _ = app.emit("update-available", version);
}

/// A release newer than this build, if the check has found one.
#[tauri::command]
pub fn update_available(state: tauri::State<UpdateState>) -> Option<String> {
    state.0.lock().ok().and_then(|known| known.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_newer_tag_is_a_release() {
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.3"), Some(v("0.1.3")));
        assert_eq!(newer_release(&v("0.1.2"), "0.2.0"), Some(v("0.2.0")));
    }

    #[test]
    fn the_same_or_an_older_tag_is_not() {
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.2"), None);
        assert_eq!(newer_release(&v("0.1.2"), "v0.1.1"), None);
    }

    #[test]
    fn a_prerelease_build_is_older_than_its_release() {
        assert_eq!(newer_release(&v("0.2.0-rc1"), "v0.2.0"), Some(v("0.2.0")));
        assert_eq!(newer_release(&v("0.2.0-rc1"), "v0.1.9"), None);
    }

    #[test]
    fn an_unparseable_tag_is_not_a_release() {
        assert_eq!(newer_release(&v("0.1.2"), "latest"), None);
        assert_eq!(newer_release(&v("0.1.2"), ""), None);
    }

    #[test]
    fn a_first_check_is_due() {
        assert!(due(None, SystemTime::now()));
    }

    #[test]
    fn a_check_is_due_after_the_interval() {
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(!due(Some(last), last + INTERVAL - Duration::from_secs(1)));
        assert!(due(Some(last), last + INTERVAL));
    }

    #[test]
    fn a_clock_that_went_backwards_is_due() {
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(due(Some(last), last - Duration::from_secs(60)));
    }

    #[test]
    fn reads_the_tag_from_a_release_body() {
        let body = r#"{"url":"x","tag_name":"v0.1.3","name":"v0.1.3","draft":false}"#;
        assert_eq!(tag_name(body).as_deref(), Some("v0.1.3"));
    }

    #[test]
    fn a_body_without_a_tag_has_none() {
        assert_eq!(tag_name(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(tag_name("<html>"), None);
    }

    #[test]
    fn a_release_build_checks_as_itself() {
        assert_eq!(check_as(false, None, &v("0.1.2")), Some(v("0.1.2")));
        assert_eq!(
            check_as(false, Some("0.0.1"), &v("0.1.2")),
            Some(v("0.1.2"))
        );
    }

    #[test]
    fn a_debug_build_checks_only_as_the_pretend_version() {
        assert_eq!(check_as(true, None, &v("0.1.2")), None);
        assert_eq!(check_as(true, Some("0.1.0"), &v("0.1.2")), Some(v("0.1.0")));
        assert_eq!(
            check_as(true, Some("v0.1.0"), &v("0.1.2")),
            Some(v("0.1.0"))
        );
        assert_eq!(check_as(true, Some("nope"), &v("0.1.2")), None);
    }

    #[test]
    #[ignore = "network: run with --ignored to check the real endpoint"]
    fn fetches_the_latest_release_tag() {
        let tag = fetch_latest_tag().expect("GitHub returned a release");
        assert!(
            newer_release(&v("0.0.0"), &tag).is_some(),
            "unparseable tag {tag}"
        );
    }
}
