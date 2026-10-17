//! Offline mode: a single switch that keeps the app from reaching the network.
//!
//! Open CAD Studio is fully usable without any connection — DWG/DXF I/O,
//! drafting, modeling, rendering, and plugins all run locally. A handful of
//! optional conveniences talk to remote services (update checks, Patreon and
//! community feeds on the Start page, the plugin marketplace, `.shx` font
//! downloads, HTTP image references). This module gates every one of them so
//! the shipped application can be provably 100 % offline.
//!
//! Resolution order (first match wins):
//! 1. `--offline` on the command line (also honoured as `OCS_OFFLINE=1`).
//! 2. The persisted `offline` setting in the user config (Settings dialog).
//! 3. The `OCS_FORCE_OFFLINE=1` environment variable — lets distributors or
//!    air-gapped deployments pin offline mode regardless of the user config.
//!
//! Once read at startup the value is cached for the process lifetime, so the
//! gate costs nothing per call and cannot race with background threads.

use std::sync::atomic::{AtomicBool, Ordering};

/// Cached decision; `false` (network allowed) until [`init`] runs.
static OFFLINE: AtomicBool = AtomicBool::new(false);

/// Environment variable that pins offline mode ahead of the saved setting.
pub const FORCE_ENV: &str = "OCS_FORCE_OFFLINE";

/// Is outbound network use disabled for this session?
#[inline]
pub fn is_offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

/// Directly set the flag (used by [`init`], by tests, and when the Settings
/// dialog applies a change mid-session).
pub fn set(offline: bool) {
    OFFLINE.store(offline, Ordering::Relaxed);
}

/// Resolve the startup value from the environment and the persisted setting.
/// Call once before any gated task is spawned; later calls are no-ops beyond
/// refreshing the cache.
pub fn init(from_settings: bool) {
    let pinned = matches!(
        std::env::var(FORCE_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    OFFLINE.store(pinned || from_settings, Ordering::Relaxed);
}

/// Command-line / environment override applied before the config file is
/// read: `--offline` anywhere in the arguments, or `OCS_OFFLINE=1`. Returns
/// `true` when either is present (the value then also seeds [`set`] so an
/// early caller sees it immediately).
pub fn apply_cli_override() -> bool {
    let env_flag = matches!(
        std::env::var("OCS_OFFLINE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let cli_flag = std::env::args().any(|arg| arg == "--offline");
    if env_flag || cli_flag {
        OFFLINE.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// Message shown in place of a feature that needs a connection while offline.
/// Kept in one spot so every gated surface tells the same story.
pub const NOTICE: &str = "Offline mode is enabled - this feature needs a network connection. Turn it off in Settings > General.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_is_round_trip() {
        // Single-threaded assertion on the shared flag; other tests only read.
        let before = is_offline();
        set(true);
        assert!(is_offline());
        set(false);
        assert!(!is_offline());
        set(before);
    }

    #[test]
    fn init_respects_saved_setting() {
        let before = is_offline();
        std::env::remove_var(FORCE_ENV);
        init(true);
        assert!(is_offline());
        init(false);
        assert!(!is_offline());
        set(before);
    }

    #[test]
    fn force_env_pins_offline() {
        let before = is_offline();
        std::env::set_var(FORCE_ENV, "1");
        init(false);
        assert!(is_offline(), "OCS_FORCE_OFFLINE=1 must win over the setting");
        std::env::remove_var(FORCE_ENV);
        init(false);
        assert!(!is_offline());
        set(before);
    }
}
