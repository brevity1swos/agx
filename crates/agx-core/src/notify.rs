//! Desktop notifications for `--live` mode — fires when the watched
//! session grows with a new error tool_result, or when it stops growing
//! for a user-specified duration. Opt-in via `--features notifications`.
//!
//! # Why feature-gated
//!
//! `notify-rust` pulls in platform-native bindings: D-Bus on Linux,
//! AppKit on macOS, WinRT on Windows. Users who don't run `agx --live`
//! don't need any of that. Gating keeps the default binary lean and
//! lets this feature co-exist with the equally optional
//! `embedding-search` and `otel-proto` features without coupling.
//!
//! # Fallback behavior
//!
//! Without the feature:
//! - `error(…)` / `idle(…)` are no-ops; they return `Ok(())` without
//!   doing anything. Callers never need a `cfg!` check.
//! - `FEATURE_DISABLED_MESSAGE` exists for the Cli dispatch in
//!   main.rs to print a one-time rebuild hint when the user passes
//!   `--notify-on-error` / `--notify-on-idle` on a feature-off build.
//!
//! With the feature:
//! - `error(step_label)` fires a notification with title "agx: error
//!   in live session" and the step label as the body. Best-effort —
//!   OS notification failures return `Err` but the live loop is
//!   expected to `.ok()` them so transient D-Bus hiccups don't crash
//!   the TUI.
//! - `idle(duration_s)` fires a notification with title "agx: live
//!   session idle" when the file has not grown for the configured
//!   duration.
//!
//! # Scope
//!
//! This module deliberately doesn't own the *when* to fire — that's
//! `tui.rs`'s business. The module is a thin wrapper over
//! `notify-rust`'s notification API so the tui.rs event loop can
//! stay format-native without dragging platform deps into its tree.

use anyhow::Result;

/// User-facing message shown the first time a notification flag is
/// used on a feature-off build. Tells users exactly how to rebuild.
/// Public because the bin crate's main.rs prints it from its CLI
/// dispatch layer.
pub const FEATURE_DISABLED_MESSAGE: &str = "--notify-on-* requires rebuilding agx with `cargo install agx --features notifications` or `cargo build --release --features notifications`";

/// Fire an error notification. Best-effort — failures return `Err` but
/// the live loop should `.ok()` them so transient OS-notification
/// hiccups never crash the TUI.
#[cfg(not(feature = "notifications"))]
pub fn error(_step_label: &str) -> Result<()> {
    Ok(())
}

/// Fire an idle notification. Best-effort.
#[cfg(not(feature = "notifications"))]
pub fn idle(_duration_s: u64) -> Result<()> {
    Ok(())
}

#[cfg(feature = "notifications")]
pub fn error(step_label: &str) -> Result<()> {
    real::error(step_label)
}

#[cfg(feature = "notifications")]
pub fn idle(duration_s: u64) -> Result<()> {
    real::idle(duration_s)
}

#[cfg(feature = "notifications")]
mod real {
    use anyhow::{Context, Result};
    use notify_rust::Notification;

    pub(super) fn error(step_label: &str) -> Result<()> {
        Notification::new()
            .summary("agx: error in live session")
            .body(step_label)
            .appname("agx")
            .show()
            .with_context(|| "failed to emit error notification")?;
        Ok(())
    }

    pub(super) fn idle(duration_s: u64) -> Result<()> {
        let body = format!("No new steps for {duration_s}s");
        Notification::new()
            .summary("agx: live session idle")
            .body(&body)
            .appname("agx")
            .show()
            .with_context(|| "failed to emit idle notification")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_disabled_message_mentions_rebuild_hint() {
        assert!(FEATURE_DISABLED_MESSAGE.contains("--features notifications"));
    }

    #[cfg(not(feature = "notifications"))]
    #[test]
    fn error_and_idle_are_noops_without_feature() {
        // Feature-off path must not error — callers rely on it to be
        // a silent no-op so the live event loop can blindly call it.
        assert!(error("anything").is_ok());
        assert!(idle(30).is_ok());
    }
}
