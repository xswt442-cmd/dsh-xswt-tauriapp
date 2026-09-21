//! Asking the registry what dsh versions exist, and installing one.
//!
//! The update question is a *harness* concern — it is about the runtime this
//! shell supervises, not about anything dsh renders — so it is asked and
//! answered in the bootstrap window, before dsh is ever shown.

use std::path::PathBuf;
use std::process::Command;

use dsh_xswt_tauriapp_core::{paths, self_update, updates};
use tauri::{AppHandle, Emitter};

use crate::shell_log;
use crate::state::{
    SelfUpdatePayload, SharedShell, UpdatePayload, EVENT_SELF_UPDATE, EVENT_UPDATE,
};

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

/// The version of this build.
///
/// Read from the crate rather than from `tauri.conf.json`: they are kept equal
/// by the release checklist, and the compiled-in value is the one actually
/// running.
pub fn shell_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Ask GitHub whether this application has a newer release.
///
/// Advisory throughout: a rate-limited API, an offline machine or a release
/// with nothing installable all end in "no prompt" rather than a failed launch.
pub fn refresh_self(app: &AppHandle, shell: &SharedShell) -> SelfUpdatePayload {
    let current = shell_version();
    let payload = match self_update::check(&current) {
        Ok(Some(available)) => {
            let dismissed = shell
                .lock()
                .map(|guard| guard.self_dismiss.is_dismissed(&available.version))
                .unwrap_or(false);
            let can_install = available.installer.is_some();
            let version = available.version.clone();
            if let Ok(mut guard) = shell.lock() {
                guard.self_pending = Some(available);
                guard.state.self_update = Some(SelfUpdatePayload {
                    current: current.clone(),
                    version: Some(version.clone()),
                    error: None,
                    can_install,
                    should_prompt: !dismissed,
                });
            }
            shell_log!(
                "[dsh-harness] self update: {current} -> {version} (installer: {can_install}, dismissed: {dismissed})"
            );
            shell
                .lock()
                .ok()
                .and_then(|guard| guard.state.self_update.clone())
                .unwrap_or_default()
        }
        Ok(None) => {
            shell_log!("[dsh-harness] self update: {current} is the newest release");
            if let Ok(mut guard) = shell.lock() {
                guard.self_pending = None;
                guard.state.self_update = Some(SelfUpdatePayload {
                    current: current.clone(),
                    ..Default::default()
                });
            }
            SelfUpdatePayload {
                current,
                ..Default::default()
            }
        }
        Err(error) => {
            // Never surfaced as a failure: not knowing about an update is not a
            // reason to bother anyone.
            shell_log!("[dsh-harness] self update check failed: {error}");
            if let Ok(mut guard) = shell.lock() {
                guard.state.self_update = Some(SelfUpdatePayload {
                    current: current.clone(),
                    error: Some(error.clone()),
                    ..Default::default()
                });
            }
            SelfUpdatePayload {
                current,
                error: Some(error),
                ..Default::default()
            }
        }
    };
    let _ = app.emit(EVENT_SELF_UPDATE, payload.clone());
    payload
}

/// Download this machine's installer, verify it, and hand it to the system.
///
/// Falls back to opening the release page when the release carries nothing this
/// machine can install. Returns what it did, for the page to show.
pub fn apply_self_update(shell: &SharedShell) -> Result<String, String> {
    let pending = shell
        .lock()
        .map_err(|_| "状态锁不可用".to_string())?
        .self_pending
        .clone()
        .ok_or_else(|| "当前没有待安装的外壳更新。".to_string())?;

    let Some(installer) = pending.installer.as_ref() else {
        crate::link::open_external(&pending.page);
        return Ok("已打开发布页。".to_string());
    };

    let directory = std::env::temp_dir()
        .join("dsh-xswt-tauriapp")
        .join("updates");
    let path = self_update::download_installer(installer, pending.checksums.as_ref(), &directory)?;
    shell_log!(
        "[dsh-harness] self update: verified {} at {}",
        installer.name,
        path.display()
    );
    crate::link::open_external(&path.display().to_string());
    // On Linux the opener is `xdg-open`, and for a `.deb` that usually means an
    // archive manager rather than an installer. Handing it over is still right —
    // the platform answers, this shell does not — but the command that does
    // install it is worth naming, because nothing in a file manager will.
    Ok(if cfg!(target_os = "linux") {
        format!(
            "已下载并校验 {}。已交给系统打开；若只是打开了归档管理器，用 sudo apt install {} 安装。",
            installer.name,
            path.display()
        )
    } else {
        format!("已下载并校验 {}，安装程序已打开。", installer.name)
    })
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
