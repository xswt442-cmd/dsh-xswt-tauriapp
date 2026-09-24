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
use std::path::Path;
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
    let (pending, directory) = {
        let guard = shell.lock().map_err(|_| "状态锁不可用".to_string())?;
        let pending = guard
            .self_pending
            .clone()
            .ok_or_else(|| "当前没有待安装的外壳更新。".to_string())?;
        (pending, guard.download_dir.clone())
    };

    let Some(installer) = pending.installer.as_ref() else {
        crate::link::open_external(&pending.page);
        return Ok("已打开发布页。".to_string());
    };

    let path = self_update::download_installer(installer, pending.checksums.as_ref(), &directory)?;
    // The directory outlives the session now, so the installers of versions this
    // one supersedes go rather than accumulating, one per release, for as long as
    // the machine lives. The one just verified is the one that stays.
    for stale in self_update::prune_installers(&directory, &installer.name) {
        shell_log!(
            "[dsh-harness] self update: removed the superseded {}",
            stale.display()
        );
    }
    shell_log!(
        "[dsh-harness] self update: verified {} at {}",
        installer.name,
        path.display()
    );
    let handover = crate::link::open_installer(&path);
    if let Err(error) = &handover {
        shell_log!("[dsh-harness] self update: could not hand the installer over ({error})");
    }
    Ok(handover_message(
        &installer.name,
        &path,
        handover,
        std::env::consts::OS,
    ))
}

/// What to tell the user after handing an installer over, given how it went.
///
/// Linux is the platform where this has to be said out loud: `xdg-open` needs a
/// handler for `.deb`, a bare WSL image ships none, and a `.deb` needs root
/// whoever opens it — so the command is named either way. The two Linux arms
/// differ in what they claim: a hand-over that worked *may* have reached an
/// archive manager instead of an installer, while one that failed opened nothing
/// at all, and reporting the second as "installer opened" is what made an update
/// look done on a machine where nothing had happened.
fn handover_message(
    installer: &str,
    path: &Path,
    handover: Result<(), String>,
    os: &str,
) -> String {
    let manual = format!("sudo apt install {}", path.display());
    match (handover, os == "linux") {
        (Ok(()), true) => format!(
            "已下载并校验 {installer}。已交给系统打开；若只是打开了归档管理器，用 {manual} 安装。"
        ),
        (Ok(()), false) => format!("已下载并校验 {installer}，安装程序已打开。"),
        (Err(error), true) => format!(
            "已下载并校验 {installer}，但系统里没有能打开它的程序（{error}）。请执行：{manual}"
        ),
        (Err(error), false) => format!(
            "已下载并校验 {installer}，但没能打开它（{error}）。安装包在 {}，可以手动运行。",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::handover_message;
    use std::path::Path;

    #[test]
    fn a_failed_handover_says_so_and_names_the_command() {
        // The WSL case this exists for: `xdg-open` exits 3 and opens nothing.
        let message = handover_message(
            "app_0.0.10_amd64.deb",
            Path::new("/tmp/dsh-xswt-tauriapp/updates/app_0.0.10_amd64.deb"),
            Err("xdg-open 退出码 Some(3)".to_string()),
            "linux",
        );
        assert!(message.contains("没有能打开它的程序"), "{message}");
        assert!(
            message.contains("sudo apt install /tmp/dsh-xswt-tauriapp/updates/"),
            "{message}"
        );
        assert!(!message.contains("安装程序已打开"), "{message}");
    }

    #[test]
    fn a_linux_handover_still_names_the_command() {
        let message = handover_message("app.deb", Path::new("/tmp/app.deb"), Ok(()), "linux");
        assert!(
            message.contains("sudo apt install /tmp/app.deb"),
            "{message}"
        );
    }

    #[test]
    fn elsewhere_the_opener_is_the_installer() {
        let message = handover_message("app.exe", Path::new("C:/x/app.exe"), Ok(()), "windows");
        assert!(message.contains("安装程序已打开"), "{message}");
        assert!(!message.contains("apt"), "{message}");
    }

    #[test]
    fn a_handover_that_failed_elsewhere_names_the_kept_file() {
        // Windows used to have no way to fail here: `explorer` was spawned and
        // never looked at, so a machine that could not open the installer read as
        // one that had. Now the shell API answers, and the answer is shown with
        // the path that is still on disk — the cache directory, not `/tmp`.
        let message = handover_message(
            "dsh-xswt-tauriapp_0.0.12_x64-setup.exe",
            Path::new("C:/Users/x/AppData/Local/com.xswt.dsh.tauri/cache/updates/setup.exe"),
            Err("系统里没有能打开它的程序".to_string()),
            "windows",
        );
        assert!(message.contains("没能打开它"), "{message}");
        assert!(message.contains("cache/updates/setup.exe"), "{message}");
        assert!(!message.contains("安装程序已打开"), "{message}");
        assert!(!message.contains("apt"), "{message}");
    }
}
