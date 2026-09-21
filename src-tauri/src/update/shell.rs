//! This application's own updates: what its GitHub releases say.
//!
//! Deliberately separate from [`super::dsh`], which is about the **dsh runtime**.
//! Two different products from two different sources — dsh is installed from
//! npm, this application is released from its own GitHub repository — so mixing
//! them would mean one version comparison answering two unrelated questions.
//!
//! The check is advisory and never fatal: a rate-limited API, an offline
//! machine or a release with nothing installable all end in "no prompt", not in
//! a failed launch.

use dsh_xswt_tauriapp_core::self_update;
use tauri::{AppHandle, Emitter};

use crate::shell_log;
use crate::state::{SelfUpdatePayload, SharedShell, EVENT_SELF_UPDATE};

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
