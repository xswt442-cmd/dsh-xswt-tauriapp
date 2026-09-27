//! Zoom for whichever window a menu item or shortcut acts on.
//!
//! Driven from Rust rather than through the webview's own zoom hotkeys: on macOS
//! and Linux those work by *injecting a polyfill into the page*, which is the one
//! thing this harness does not do.

use tauri::AppHandle;

use dsh_xswt_tauriapp_core::zoom;

use crate::guest::window::target_window;
use crate::shell_log;
use crate::state::SharedShell;

/// One zoom step. The bounds live in `dsh_core::zoom`, next to the rule for the
/// value to start at and the file it is remembered in.
const ZOOM_STEP: f64 = 1.1;

// ── the session hand-off ───────────────────────────────────────────────────

/// Move the factor by `step`, apply it, and remember it unless the session is
/// pinned.
///
/// Driven from Rust rather than the webview's own zoom hotkeys: on macOS and
/// Linux that setting works by *injecting a polyfill into the page*, which is
/// the one thing this harness does not do.
///
/// The factor is remembered as well as applied. On a display whose applications
/// are scaled — Windows at 125%, say — a WSLg session renders every window at
/// scale 1, so the factor is compensating for the machine rather than for the
/// moment; asking for it again on every launch is the whole of that complaint.
///
/// [`zoom::ZOOM_ENV`] pins the factor *for one session*, and the environment
/// outranks the memory — so while it is set, the memory is left alone. Writing it
/// anyway would promote a one-off pin into the setting every later launch
/// inherits, which is the opposite of what a pin is for.
fn apply(app: &AppHandle, shell: &SharedShell, step: impl FnOnce(f64) -> f64) {
    let factor = match shell.lock() {
        Ok(mut guard) => {
            let next = zoom::clamp(step(guard.zoom)).unwrap_or(guard.zoom);
            guard.zoom = next;
            if std::env::var_os(zoom::ZOOM_ENV).is_none() {
                if let Err(error) = guard.zoom_memory.remember(next) {
                    shell_log!("[dsh-harness] could not remember the zoom factor: {error}");
                }
            }
            next
        }
        Err(_) => return,
    };
    if let Some(window) = target_window(app) {
        let _ = window.set_zoom(factor);
    }
}

/// Zoom in one step.
pub fn zoom_in(app: &AppHandle, shell: &SharedShell) {
    apply(app, shell, |factor| factor * ZOOM_STEP);
}

/// Zoom out one step.
pub fn zoom_out(app: &AppHandle, shell: &SharedShell) {
    apply(app, shell, |factor| factor / ZOOM_STEP);
}

/// Return the zoom factor to 1.0, and remember that too — a reset is a choice.
pub fn zoom_reset(app: &AppHandle, shell: &SharedShell) {
    apply(app, shell, |_| 1.0);
}
