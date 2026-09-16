//! The IPC surface.
//!
//! Every one of these is reachable only from the bootstrap page: the capability
//! that grants IPC is scoped to that window's label, and the dsh window is a
//! remote origin that no capability covers. That is deliberate — the wrapped app
//! must never gain access to the harness's command surface.

use tauri::{AppHandle, State};

use crate::state::{self, SharedShell, ShellState};
use crate::update::{self, UpdatePayload};
use crate::{guest, shell_log};

/// The current snapshot, for the page to pull after it has attached listeners.
#[tauri::command]
pub fn get_state(shell: State<'_, SharedShell>) -> ShellState {
    state::snapshot(shell.inner())
}

/// Hand the screen over to dsh.
///
/// Called once the user has seen — or skipped — the update dialog. The session
/// cookie is already in the jar by then: the worker wrote it before it announced
/// the session, so this does nothing but build the window and hide the page.
///
/// `async` on purpose. A synchronous command runs inside the webview's own IPC
/// callback, and building a second webview from there deadlocks on Windows
/// (wry#583) — the window is created and then never handed back, so the guest
/// stays hidden behind the splash forever. An `async` command runs off the main
/// thread, so `WebviewWindowBuilder::build` asks the main thread for the window
/// and waits, instead of re-entering it.
#[tauri::command]
pub async fn open_dsh(app: AppHandle, shell: State<'_, SharedShell>) -> Result<(), String> {
    guest::spawn(&app, shell.inner())
}

/// Re-run the update check on demand (the dialog's "重新检查").
#[tauri::command]
pub fn check_updates(app: AppHandle, shell: State<'_, SharedShell>) -> UpdatePayload {
    update::refresh(&app, shell.inner())
}

/// Add a version to the do-not-remind list and return the refreshed state.
#[tauri::command]
pub fn dismiss_version(
    shell: State<'_, SharedShell>,
    version: String,
) -> Result<ShellState, String> {
    let mut guard = shell.lock().map_err(|_| "状态锁不可用".to_string())?;
    guard.store.dismiss(&version)?;
    if let Some(report) = guard.state.update.as_mut() {
        if report
            .candidate
            .as_ref()
            .is_some_and(|entry| entry.version == version)
        {
            report.candidate_dismissed = true;
        }
    }
    Ok(guard.state.clone())
}

/// Forget every do-not-remind entry.
#[tauri::command]
pub fn clear_dismissed(shell: State<'_, SharedShell>) -> Result<ShellState, String> {
    let mut guard = shell.lock().map_err(|_| "状态锁不可用".to_string())?;
    guard.store.clear()?;
    if let Some(report) = guard.state.update.as_mut() {
        report.candidate_dismissed = false;
    }
    Ok(guard.state.clone())
}

/// The versions currently on the do-not-remind list.
#[tauri::command]
pub fn dismissed_versions(shell: State<'_, SharedShell>) -> Vec<String> {
    shell
        .lock()
        .map(|guard| guard.store.dismissed.clone())
        .unwrap_or_default()
}

/// Install one dsh version globally, then restart so the new launcher is used.
#[tauri::command]
pub fn apply_update(app: AppHandle, version: String) -> Result<(), String> {
    update::install(&app, &version)
}

/// Restart the harness without updating anything.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart()
}

/// Where the do-not-remind list lives, for display in the UI.
#[tauri::command]
pub fn dismiss_path(shell: State<'_, SharedShell>) -> Option<String> {
    shell
        .lock()
        .ok()
        .and_then(|guard| guard.dismiss_path.as_ref().map(|p| p.display().to_string()))
}

/// Diagnostics relayed from the bootstrap page's own error handlers.
///
/// The page is the shell's own, so reporting from it is ordinary application
/// code — nothing is ever injected into the dsh origin. A page that fails to run
/// is the one failure the harness cannot report any other way, and a packaged
/// GUI app has no terminal to print to otherwise.
#[tauri::command]
pub fn page_diag(stage: String, message: String) {
    shell_log!("[dsh-harness] page {stage}: {message}");
}
