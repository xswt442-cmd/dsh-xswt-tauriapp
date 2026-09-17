//! The IPC surface.
//!
//! Every one of these is reachable only from the bootstrap page: the capability
//! that grants IPC is scoped to that window's label, and the dsh window is a
//! remote origin that no capability covers. That is deliberate — the wrapped app
//! must never gain access to the harness's command surface.

use dsh_xswt_tauriapp_core::server::{self, PortChoice};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::{self, SharedShell, ShellState};
use crate::update::{self, SelfUpdatePayload, UpdatePayload};
use crate::{bootstrap, guest, shell_log};

/// What the port in the dialog would do, classified.
///
/// Only the classification crosses the bridge: the wording belongs to the page,
/// which also styles the three cases differently.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortKind {
    /// A dsh is already running there and will be entered.
    Reuse,
    /// Nothing is listening there; a server will be started.
    Start,
    /// Something else owns the port.
    Occupied,
    /// A dsh is there, but this machine cannot enter its session.
    Foreign,
    /// Below the port a desktop application may bind.
    TooLow,
}

/// The answer to "what if I used this port?", with the port it is about.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PortVerdict {
    /// What would happen.
    pub kind: PortKind,
    /// The port that was asked about.
    pub port: u16,
}

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

/// Classify a port before the user commits to it.
///
/// `async` for the same reason `open_dsh` is: it makes blocking network calls,
/// and a synchronous command would run them inside the webview's own IPC
/// callback, on the main thread, with the window frozen for as long as they take.
#[tauri::command]
pub async fn check_port(port: u16) -> PortVerdict {
    let kind = match server::check_port(port) {
        PortChoice::Reuse(_) => PortKind::Reuse,
        PortChoice::Start => PortKind::Start,
        PortChoice::Occupied => PortKind::Occupied,
        PortChoice::Foreign => PortKind::Foreign,
        PortChoice::TooLow => PortKind::TooLow,
    };
    PortVerdict { kind, port }
}

/// Start — or adopt — a server on the port the user chose.
///
/// Returns as soon as the work is under way: spawning a server can take its whole
/// boot, so the outcome arrives as `shell://ready` or `shell://error` instead of
/// as this call's return value, and the page shows progress meanwhile.
#[tauri::command]
pub async fn start_server(
    app: AppHandle,
    shell: State<'_, SharedShell>,
    port: u16,
) -> Result<(), String> {
    let shared = shell.inner().clone();
    // Only a port the user actually named is remembered. Accepting the suggested
    // default is not a preference, and remembering it would pin the suggestion
    // to whatever the first launch happened to pick — the point of the default
    // is that it keeps tracking the first free port.
    let named = shared
        .lock()
        .map(|guard| guard.state.default_port != Some(port))
        .unwrap_or(false);
    if named {
        // Before anything can fail, and kept even if it does: a port this user
        // asked for is the port they want next time too.
        if let Ok(mut guard) = shared.lock() {
            if let Err(error) = guard.port_memory.remember(port) {
                shell_log!("[dsh-harness] could not remember port {port}: {error}");
            }
        }
    }
    std::thread::spawn(move || bootstrap::start(app, shared, port));
    Ok(())
}

/// Re-run the update check on demand (the dialog's "重新检查").
#[tauri::command]
pub fn check_updates(app: AppHandle, shell: State<'_, SharedShell>) -> UpdatePayload {
    update::refresh(&app, shell.inner())
}

/// Re-run the check for a newer build of this application.
#[tauri::command]
pub fn check_self_update(app: AppHandle, shell: State<'_, SharedShell>) -> SelfUpdatePayload {
    update::refresh_self(&app, shell.inner())
}

/// Stop reminding about one build of this application.
///
/// A separate command from `dismiss_version`, and a separate store behind it:
/// dsh versions and application versions are different namespaces.
#[tauri::command]
pub fn dismiss_self_version(shell: State<'_, SharedShell>, version: String) -> Result<(), String> {
    let mut guard = shell.lock().map_err(|_| "状态锁不可用".to_string())?;
    guard.self_dismiss.dismiss(&version)?;
    if let Some(payload) = guard.state.self_update.as_mut() {
        if payload.version.as_deref() == Some(version.as_str()) {
            payload.should_prompt = false;
        }
    }
    Ok(())
}

/// Download this machine's installer, verify it, and open it.
///
/// `async` for the same reason `open_dsh` is: it reaches the network, and a
/// synchronous command would do that inside the webview's IPC callback.
#[tauri::command]
pub async fn apply_self_update(shell: State<'_, SharedShell>) -> Result<String, String> {
    let shared = shell.inner().clone();
    update::apply_self_update(&shared)
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
