//! Asking the registry what dsh versions exist, and installing one.

use std::path::PathBuf;
use std::process::Command;

use dsh_xswt_tauriapp_core::{paths, updates};
use tauri::{AppHandle, Emitter};

use crate::shell_log;
use crate::state::{SharedShell, UpdatePayload, EVENT_UPDATE};

/// Ask the registry what exists and publish the answer.
pub fn refresh(app: &AppHandle, shell: &SharedShell) -> UpdatePayload {
    let current = paths::installed_version().unwrap_or_else(|| "0.0.0".to_string());
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
///
/// The prefix now comes from the launcher's own layout on either platform, so a
/// Windows install answers too — npm's prefix is `<prefix>\node_modules` there,
/// with no `lib` level to walk to. See [`paths::node_prefix_of`].
fn npm_for_dsh() -> Option<PathBuf> {
    let launcher = std::fs::canonicalize(paths::resolve_dsh_bin()?).ok()?;
    let prefix = paths::node_prefix_of(&launcher)?;
    // The prefix holds npm at its root or under `bin`, which are the same two
    // shapes that named it — and the one that matched is the one that answers.
    paths::npm_in(&prefix).or_else(|| paths::npm_in(&prefix.join("bin")))
}

/// The `npm` to install through: the one that owns dsh, else the one beside the
/// resolved `node`, else nothing.
fn npm_for_update() -> Result<PathBuf, String> {
    if let Some(npm) = npm_for_dsh() {
        return Ok(npm);
    }
    let node = paths::resolve_node()
        .ok_or_else(|| "未找到 node，也无法从 dsh 安装位置推断 npm。".to_string())?;
    let bin_dir = node.parent().ok_or_else(|| "node 路径异常".to_string())?;
    paths::npm_in(bin_dir).ok_or_else(|| format!("未在 {} 找到 npm", bin_dir.display()))
}

/// Install one dsh version globally, then restart so the new launcher is used.
///
/// The install runs through the npm that owns the dsh being updated, so a
/// version-managed node (nvm, fnm) updates its own global prefix rather than
/// whichever prefix some other npm on `PATH` happens to own.
pub fn install(app: &AppHandle, version: &str) -> Result<(), String> {
    let npm = npm_for_update()?;
    // Kept for the message: on Windows the argv is a re-parsed command line, and
    // quoting that back at the user would explain less than the install it means.
    let argv = updates::install_argv(version)?;
    let spec = updates::install_command(&npm, version, std::env::consts::OS)?;

    let mut command = Command::new(install_program(&spec));
    command.args(&spec.args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // An npm install is plumbing; a console window that flashes for its
        // duration is not something the user asked for.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .map_err(|error| format!("执行 npm 失败：{error}"))?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} {} 失败（退出码 {:?}）\n{}\n{}",
            npm.display(),
            argv.join(" "),
            output.status.code(),
            stdout.trim(),
            stderr.trim()
        ));
    }
    app.restart()
}

/// The interpreter to run an install with.
///
/// `cmd.exe` is taken from the environment when the session names one, so the
/// interpreter Windows ships is the one used rather than whichever `cmd.exe` the
/// search path happens to find.
fn install_program(spec: &updates::InstallCommand) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        if spec.program == std::path::Path::new("cmd.exe") {
            if let Some(comspec) = std::env::var_os("ComSpec") {
                return comspec;
            }
        }
    }
    spec.program.clone().into_os_string()
}
