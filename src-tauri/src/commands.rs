//! The IPC surface.
//!
//! Every one of these is reachable only from the bootstrap page: the capability
//! that grants IPC is scoped to that window's label, and the dsh window is a
//! remote origin that no capability covers. That is deliberate — the wrapped app
//! must never gain access to the harness's command surface.

use dsh_xswt_tauriapp_core::server;
use tauri::{AppHandle, State};

use crate::state::{
    self, kind_of, PortVerdict, SelfUpdatePayload, SharedShell, ShellState, UpdatePayload,
};
use crate::update;
use crate::{bootstrap, guest, shell_log};

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
    PortVerdict {
        kind: kind_of(&server::check_port(port)),
        port,
    }
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
    typed: bool,
) -> Result<(), String> {
    let shared = shell.inner().clone();
    // Only a port the user actually named is remembered. Accepting the suggested
    // default is not a preference, and remembering it would pin the suggestion to
    // whatever the first launch happened to pick — the point of the default is
    // that it keeps tracking the first free port.
    //
    // `typed` is a parameter rather than `port != default_port`, which cannot tell
    // "I want 3080" from "I did not care and 3080 was offered": the page knows
    // which field the value came from, and this is the only place that does.
    if typed {
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
///
/// `async` for the same reason `check_port` is: the registry call blocks for as
/// long as its timeout allows, and a synchronous command runs that inside the
/// webview's IPC callback, on the main thread, with the window frozen.
///
/// `Result` rather than the payload itself, because a `State` argument makes this
/// a command with a reference input, and Tauri requires those to be fallible.
#[tauri::command]
pub async fn check_updates(
    app: AppHandle,
    shell: State<'_, SharedShell>,
) -> Result<UpdatePayload, String> {
    // The shared handle is cloned out before the first await: a future must own
    // everything it holds, and `State` is a borrow of the app's managed state.
    let shared = shell.inner().clone();
    Ok(update::refresh(&app, &shared))
}

/// Re-run the check for a newer build of this application.
#[tauri::command]
pub async fn check_self_update(
    app: AppHandle,
    shell: State<'_, SharedShell>,
) -> Result<SelfUpdatePayload, String> {
    let shared = shell.inner().clone();
    Ok(update::refresh_self(&app, &shared))
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
///
/// `async` for the same reason, and `spawn_blocking` on top of it: `npm install
/// -g` runs for minutes, and an async command would still hold one of the async
/// runtime's worker threads for all of it. Handing the wait to the blocking pool
/// keeps both the main thread and the worker pool free.
#[tauri::command]
pub async fn apply_update(app: AppHandle, version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || update::install(&app, &version))
        .await
        .map_err(|error| format!("更新任务未能完成：{error}"))?
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
