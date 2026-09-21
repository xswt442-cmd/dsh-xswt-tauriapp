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

/// Step the zoom factor, clamping it to something usable.
///
/// Driven from Rust rather than the webview's own zoom hotkeys: on macOS and
/// Linux that setting works by *injecting a polyfill into the page*, which is
/// the one thing this harness does not do.
///
/// The factor is remembered as well as applied. On a display whose applications
/// are scaled — Windows at 125%, say — a WSLg session renders every window at
/// scale 1, so the factor is compensating for the machine rather than for the
/// moment; asking for it again on every launch is the whole of that complaint.
fn zoom_by(app: &AppHandle, shell: &SharedShell, factor: f64) {
    let zoom = match shell.lock() {
        Ok(mut guard) => {
            let next = zoom::clamp(guard.zoom * factor).unwrap_or(1.0);
            guard.zoom = next;
            if let Err(error) = guard.zoom_memory.remember(next) {
                shell_log!("[dsh-harness] could not remember the zoom factor: {error}");
            }
            next
        }
        Err(_) => return,
    };
    if let Some(window) = target_window(app) {
        let _ = window.set_zoom(zoom);
    }
}

/// Zoom in one step.
pub fn zoom_in(app: &AppHandle, shell: &SharedShell) {
    zoom_by(app, shell, ZOOM_STEP);
}

/// Zoom out one step.
pub fn zoom_out(app: &AppHandle, shell: &SharedShell) {
    zoom_by(app, shell, 1.0 / ZOOM_STEP);
}

/// Return the zoom factor to 1.0, and remember that too — a reset is a choice.
pub fn zoom_reset(app: &AppHandle, shell: &SharedShell) {
    if let Ok(mut guard) = shell.lock() {
        guard.zoom = 1.0;
        if let Err(error) = guard.zoom_memory.remember(1.0) {
            shell_log!("[dsh-harness] could not remember the zoom factor: {error}");
        }
    }
    if let Some(window) = target_window(app) {
        let _ = window.set_zoom(1.0);
    }
}
