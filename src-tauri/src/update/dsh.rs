//! Asking the registry what dsh versions exist, and installing one.

use std::path::PathBuf;
use std::process::Command;

use dsh_xswt_tauriapp_core::{console, paths, updates};
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

/// The `npm` to install through: the one that owns dsh, else the one beside the
/// resolved `node`, else nothing.
///
/// The first answer comes from [`paths::npm_for_dsh`], which is the one place
/// that decides it — derived from the launcher rather than from `PATH`, whose
/// first `node` is often an older system one with a prefix of its own.
fn npm_for_update() -> Result<PathBuf, String> {
    if let Some(npm) = paths::npm_for_dsh() {
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
        // Not lossy: npm writes UTF-8, Windows writes its own messages in the
        // console's code page, and the second kind is what an install actually
        // fails with — decoded as UTF-8 those arrived as replacement characters.
        let stdout = console::decode(&output.stdout);
        let stderr = console::decode(&output.stderr);
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
