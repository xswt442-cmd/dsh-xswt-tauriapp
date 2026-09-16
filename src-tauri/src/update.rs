//! Asking the registry what dsh versions exist, and installing one.
//!
//! The update question is a *harness* concern — it is about the runtime this
//! shell supervises, not about anything dsh renders — so it is asked and
//! answered in the bootstrap window, before dsh is ever shown.

use std::path::PathBuf;
use std::process::Command;

use dsh_xswt_tauriapp_core::{server, updates};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::shell_log;
use crate::state::{SharedShell, EVENT_UPDATE};

/// Result of one update check, as delivered to the bootstrap page.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdatePayload {
    /// Installed version at check time.
    pub current: String,
    /// The channel report, when the registry answered.
    pub report: Option<updates::UpdateReport>,
    /// Why the check failed, when it did.
    pub error: Option<String>,
    /// Whether the launch popup should appear.
    pub should_prompt: bool,
}

/// Ask the registry what exists and publish the answer.
pub fn refresh(app: &AppHandle, shell: &SharedShell) -> UpdatePayload {
    let current = server::installed_version().unwrap_or_else(|| "0.0.0".to_string());
    let store = shell
        .lock()
        .map(|guard| guard.store.clone())
        .unwrap_or_default();

    let payload = match updates::check(&current, &store) {
        Ok(report) => UpdatePayload {
            current: current.clone(),
            should_prompt: report.should_prompt(),
            report: Some(report),
            error: None,
        },
        Err(error) => UpdatePayload {
            current: current.clone(),
            should_prompt: false,
            report: None,
            error: Some(error),
        },
    };

    if let Ok(mut guard) = shell.lock() {
        guard.state.current_version = Some(current);
        guard.state.update = payload.report.clone();
        guard.state.update_error = payload.error.clone();
    }
    shell_log!(
        "[dsh-harness] update check: current={} candidate={:?} dismissed={:?} should_prompt={}",
        payload.current,
        payload
            .report
            .as_ref()
            .and_then(|r| r.candidate.as_ref().map(|c| c.version.clone())),
        payload
            .report
            .as_ref()
            .map(|r| r.candidate_dismissed)
            .unwrap_or(false),
        payload.should_prompt,
    );
    let _ = app.emit(EVENT_UPDATE, payload.clone());
    payload
}

/// The `npm` that owns the dsh this shell is actually running against.
///
/// Derived from the launcher rather than from `PATH`. A desktop launch inherits
/// a minimal `PATH`, where the first `node` is often an older system one — on
/// this machine `/usr/bin/node` is v18 and its npm installs into `/usr/local`,
/// which is both a different prefix from the dsh in use and not writable by an
/// ordinary user. Installing there updates nothing this shell can see, or fails
/// outright, which is exactly what a launched-from-the-menu update used to do.
fn npm_for_dsh() -> Option<PathBuf> {
    let launcher = std::fs::canonicalize(server::resolve_dsh_bin()?).ok()?;
    let bin_dir = server::node_prefix_of(&launcher)?.join("bin");
    npm_in(&bin_dir)
}

/// The `npm` to install through: the one that owns dsh, else the one beside the
/// resolved `node`, else nothing.
fn npm_for_update() -> Result<PathBuf, String> {
    if let Some(npm) = npm_for_dsh() {
        return Ok(npm);
    }
    let node = server::resolve_node()
        .ok_or_else(|| "未找到 node，也无法从 dsh 安装位置推断 npm。".to_string())?;
    let bin_dir = node.parent().ok_or_else(|| "node 路径异常".to_string())?;
    npm_in(bin_dir).ok_or_else(|| format!("未在 {} 找到 npm", bin_dir.display()))
}

/// The npm executable in `dir`, whichever name the platform uses.
fn npm_in(dir: &std::path::Path) -> Option<PathBuf> {
    ["npm", "npm.cmd", "npm.exe"]
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Install one dsh version globally, then restart so the new launcher is used.
///
/// The install runs through the npm that owns the dsh being updated, so a
/// version-managed node (nvm, fnm) updates its own global prefix rather than
/// whichever prefix some other npm on `PATH` happens to own.
pub fn install(app: &AppHandle, version: &str) -> Result<(), String> {
    let npm = npm_for_update()?;

    let args = updates::install_argv(version);
    let output = Command::new(&npm)
        .args(&args)
        .output()
        .map_err(|error| format!("执行 npm 失败：{error}"))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} {} 失败（退出码 {:?}）\n{}\n{}",
            npm.display(),
            args.join(" "),
            output.status.code(),
            stdout.trim(),
            stderr.trim()
        ));
    }
    app.restart()
}
