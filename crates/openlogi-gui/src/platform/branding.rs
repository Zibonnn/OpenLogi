//! Product name and bundle identity — release vs dev builds.

#![cfg_attr(
    target_os = "macos",
    expect(unsafe_code, reason = "NSBundle bundleIdentifier FFI for dev vs release detection")
)]

/// User-visible application name.
#[must_use]
pub fn display_name() -> &'static str {
    if is_dev_build() {
        "OpenLogi Dev"
    } else {
        "OpenLogi"
    }
}

/// `true` when running the throwaway dev `.app` (or a debug raw binary).
#[must_use]
pub fn is_dev_build() -> bool {
    #[cfg(target_os = "macos")]
    {
        if bundle_identifier().is_some_and(|id| id.ends_with(".dev")) {
            return true;
        }
    }
    cfg!(debug_assertions)
}

/// LaunchAgent label — distinct for dev so login-item prefs don't clobber release.
#[must_use]
pub fn launch_agent_label() -> &'static str {
    if is_dev_build() {
        "org.openlogi.openlogi.dev"
    } else {
        "org.openlogi.openlogi"
    }
}

/// Single-instance lock filename under the config dir.
#[must_use]
pub fn instance_lock_file() -> &'static str {
    if is_dev_build() {
        "openlogi-dev.lock"
    } else {
        "openlogi.lock"
    }
}

/// Window / menu title prefix, optionally with a section suffix.
#[must_use]
pub fn window_title(suffix: Option<&str>) -> String {
    match suffix {
        Some(part) => format!("{} — {part}", display_name()),
        None => display_name().to_string(),
    }
}

#[cfg(target_os = "macos")]
fn bundle_identifier() -> Option<String> {
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let bundle: id = msg_send![class!(NSBundle), mainBundle];
        if bundle == nil {
            return None;
        }
        let ident: id = msg_send![bundle, bundleIdentifier];
        if ident == nil {
            return None;
        }
        let raw: *const i8 = msg_send![ident, UTF8String];
        if raw.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned())
    }
}
