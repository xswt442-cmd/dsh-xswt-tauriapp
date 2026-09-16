//! Tauri shell for DeepSeek Harness.
//!
//! The shell does not touch dsh's source. On launch it finds an already-running
//! `dsh web` UI or starts one, embeds it, and meanwhile asks the registry
//! whether a newer dsh exists. That check is what the splash page turns into
//! the three-column update dialog.
//!
//! Threading: the window is created in `setup` (it has to be, so the
//! navigation hooks can be attached) and shows the bundled splash page first.
//! One worker thread owns the slow work (discovery, server start, registry
//! fetch) and reports progress by emitting events; the page decides when to
//! navigate the webview to the dsh UI. Doing the blocking work off the main
//! thread keeps the window painting instead of showing a blank frame for the
//! whole cold start.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use dsh_xswt_tauriapp_core::{server, updates};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

/// Progress text while the shell comes up.
const EVENT_STATUS: &str = "shell://status";
/// The dsh UI is reachable; carries the full [`ShellState`].
const EVENT_READY: &str = "shell://ready";
/// Bringing the UI up failed; carries the full [`ShellState`].
const EVENT_ERROR: &str = "shell://error";
/// Result of an update check; carries an [`UpdatePayload`].
const EVENT_UPDATE: &str = "shell://update";

/// Log a shell event.
///
/// On in debug builds, and in release builds when `DSH_SHELL_DEBUG` is set —
/// a packaged GUI app has no terminal, so "why did it not hand over" would
/// otherwise be unanswerable in the field.
macro_rules! shell_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) || std::env::var_os("DSH_SHELL_DEBUG").is_some() {
            eprintln!($($arg)*);
        }
    };
}

/// Everything the splash page needs to render, in one serialisable snapshot.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ShellState {
    /// `starting` | `ready` | `failed`.
    pub phase: String,
    /// Human-readable progress line.
    pub message: String,
    /// The dsh UI URL, once known.
    pub url: Option<String>,
    /// Port the UI is served on.
    pub port: Option<u16>,
    /// Whether an existing server was reused rather than started.
    pub reused: bool,
    /// Fatal startup error, when `phase` is `failed`.
    pub error: Option<String>,
    /// The installed dsh version.
    pub current_version: Option<String>,
    /// Directory holding `server-<port>.{out,err}.log`, shown on failure.
    pub log_dir: Option<String>,
    /// The latest update check result.
    pub update: Option<updates::UpdateReport>,
    /// Why the last update check failed, if it did.
    pub update_error: Option<String>,
}

/// Result of one update check, as delivered to the page.
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

/// Process-wide shell state.
#[derive(Default)]
struct Shell {
    state: ShellState,
    store: updates::DismissStore,
    /// Where the do-not-remind list is persisted.
    dismiss_path: Option<PathBuf>,
    /// The shell's own page, captured at setup so a failed hand-off can come
    /// back to a page that can explain itself.
    shell_url: Option<String>,
    /// How many times the hand-off has been retried.
    handoff_retries: u32,
}

type SharedShell = Arc<Mutex<Shell>>;

fn snapshot(shell: &SharedShell) -> ShellState {
    shell
        .lock()
        .map(|guard| guard.state.clone())
        .unwrap_or_default()
}

/// Record and broadcast a progress line.
fn set_message(app: &AppHandle, shell: &SharedShell, message: &str) {
    let snap = match shell.lock() {
        Ok(mut guard) => {
            guard.state.message = message.to_string();
            guard.state.clone()
        }
        Err(_) => return,
    };
    let _ = app.emit(EVENT_STATUS, snap);
}

/// Ask the registry what exists and publish the answer.
fn refresh_update(app: &AppHandle, shell: &SharedShell) -> UpdatePayload {
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
        "[dsh-shell] update check: current={} candidate={:?} dismissed={:?} should_prompt={}",
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

/// Worker thread: bring the UI up, then check for updates.
fn bootstrap(app: AppHandle, shell: SharedShell) {
    set_message(&app, &shell, "正在查找已运行的 dsh 服务…");

    let progress_app = app.clone();
    let progress_shell = shell.clone();
    let launch = server::launch_with_progress(server::BOOT_TIMEOUT_SECS, move |message| {
        set_message(&progress_app, &progress_shell, message);
    });

    match launch {
        Ok(launch) => {
            let url = launch.url().to_string();
            let port = launch.port();
            let reused = matches!(launch, server::Launch::Reused { .. });
            if let Ok(mut guard) = shell.lock() {
                guard.state.phase = "ready".into();
                guard.state.url = Some(url);
                guard.state.port = Some(port);
                guard.state.reused = reused;
                guard.state.message = if reused {
                    "已复用运行中的 dsh 服务".into()
                } else {
                    "服务已就绪".into()
                };
            }
            let _ = app.emit(EVENT_READY, snapshot(&shell));
            refresh_update(&app, &shell);
        }
        Err(error) => {
            if let Ok(mut guard) = shell.lock() {
                guard.state.phase = "failed".into();
                guard.state.message = "启动失败".into();
                guard.state.error = Some(error);
            }
            let _ = app.emit(EVENT_ERROR, snapshot(&shell));
        }
    }
}

/// The current snapshot, for the page to pull after it has attached listeners.
#[tauri::command]
fn get_state(shell: State<'_, SharedShell>) -> ShellState {
    snapshot(shell.inner())
}

/// Point the webview at the dsh UI. Called once the user has seen — or skipped
/// — the update dialog.
#[tauri::command]
fn open_dsh(window: WebviewWindow, shell: State<'_, SharedShell>) -> Result<(), String> {
    let url = shell
        .lock()
        .map_err(|_| "状态锁不可用".to_string())?
        .state
        .url
        .clone()
        .ok_or_else(|| "dsh 服务尚未就绪".to_string())?;
    let parsed: tauri::Url = url.parse().map_err(|error| format!("URL 无效：{error}"))?;
    let outcome = window.navigate(parsed);
    shell_log!(
        "[dsh-shell] open_dsh {} -> {}",
        url,
        match &outcome {
            Ok(()) => "ok".to_string(),
            Err(error) => format!("失败: {error}"),
        }
    );
    outcome.map_err(|error| format!("导航失败：{error}"))
}

/// Re-run the update check on demand (the dialog's "重新检查").
#[tauri::command]
fn check_updates(app: AppHandle, shell: State<'_, SharedShell>) -> UpdatePayload {
    refresh_update(&app, shell.inner())
}

/// Add a version to the do-not-remind list and return the refreshed state.
#[tauri::command]
fn dismiss_version(shell: State<'_, SharedShell>, version: String) -> Result<ShellState, String> {
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
fn clear_dismissed(shell: State<'_, SharedShell>) -> Result<ShellState, String> {
    let mut guard = shell.lock().map_err(|_| "状态锁不可用".to_string())?;
    guard.store.clear()?;
    if let Some(report) = guard.state.update.as_mut() {
        report.candidate_dismissed = false;
    }
    Ok(guard.state.clone())
}

/// The versions currently on the do-not-remind list.
#[tauri::command]
fn dismissed_versions(shell: State<'_, SharedShell>) -> Vec<String> {
    shell
        .lock()
        .map(|guard| guard.store.dismissed.clone())
        .unwrap_or_default()
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
    ["npm", "npm.cmd", "npm.exe"]
        .iter()
        .map(|name| bin_dir.join(name))
        .find(|candidate| candidate.is_file())
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
    ["npm", "npm.cmd", "npm.exe"]
        .iter()
        .map(|name| bin_dir.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("未在 {} 找到 npm", bin_dir.display()))
}

/// Install one dsh version globally, then restart so the new launcher is used.
///
/// The install runs through the npm that owns the dsh being updated, so a
/// version-managed node (nvm, fnm) updates its own global prefix rather than
/// whichever prefix some other npm on `PATH` happens to own.
#[tauri::command]
fn apply_update(app: AppHandle, version: String) -> Result<(), String> {
    let npm = npm_for_update()?;

    let args = updates::install_argv(&version);
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

/// Restart the shell without updating anything.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart()
}

/// Where the do-not-remind list lives, for display in the UI.
#[tauri::command]
fn dismiss_path(shell: State<'_, SharedShell>) -> Option<String> {
    shell
        .lock()
        .ok()
        .and_then(|guard| guard.dismiss_path.as_ref().map(|p| p.display().to_string()))
}

/// The window's own scheme, plus anything on loopback, stays inside the
/// webview: the bundled splash page (`tauri://localhost/…`) and the dsh UI
/// (`http://127.0.0.1:<port>/…`). Everything else is a link out.
fn is_internal(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        "http" | "https" => url.host_str().is_some_and(|host| {
            host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host.ends_with(".localhost")
        }),
        _ => false,
    }
}

/// Injected into the page before anything else runs.
///
/// The splash page talks to Rust through `window.__TAURI__`, which only exists
/// when `withGlobalTauri` is on. If that bridge is missing, or the module
/// bundle fails to load, the page dies silently and the shell looks like it
/// simply stopped — so this reporter goes straight to `__TAURI_INTERNALS__`,
/// which is always present, and surfaces the failure on stderr.
const DIAG_SCRIPT: &str = r#"
(function () {
  // Diagnostics belong to the shell's own page. On the dsh origin there is no
  // IPC to call, and trying anyway is worse than useless: `invoke` returns a
  // promise that never settles there, and an uncaught rejection would re-enter
  // the rejection handler below in a loop.
  var isShellPage = location.protocol === 'tauri:' || location.hostname === 'tauri.localhost';

  var reports = 0;
  var report = function (stage, message) {
    if (reports > 20) return;
    reports += 1;
    try {
      var pending = window.__TAURI_INTERNALS__.invoke('page_diag', {
        stage: stage,
        message: String(message),
      });
      if (pending && typeof pending.catch === 'function') pending.catch(function () {});
    } catch (error) { /* no bridge at all */ }
  };

  if (isShellPage) {
    window.addEventListener('error', function (event) {
      var target = event.target;
      if (target && target !== window && (target.src || target.href)) {
        report('resource-error', (target.tagName || '?') + ' ' + (target.src || target.href));
      } else {
        report('error', (event.message || '?') + ' @ ' + (event.filename || '?') + ':' + (event.lineno || 0));
      }
    }, true);
    window.addEventListener('unhandledrejection', function (event) {
      var reason = event.reason;
      report('unhandledrejection', reason && reason.message ? reason.message : reason);
    });
  }

  // This window has no browser chrome, so it also has no reload — and dsh needs
  // one: the content font size and the theme's boot values are written into the
  // index when the host renders it, and nothing applies them afterwards. The
  // shortcut the browser would have provided is restored here.
  window.addEventListener('keydown', function (event) {
    var reload = event.key === 'F5' ||
      ((event.ctrlKey || event.metaKey) && (event.key === 'r' || event.key === 'R'));
    if (reload) {
      event.preventDefault();
      location.reload();
    }
  }, true);

  document.addEventListener('DOMContentLoaded', function () {
    if (isShellPage) {
      report('dom-ready', 'hasTauriGlobal=' + (typeof window.__TAURI__) +
        ' hasInternals=' + (typeof window.__TAURI_INTERNALS__));
    }
    // Runs on every page, including the dsh UI. Landing on dsh's auth page
    // means the hand-off arrived without a session; without this the window just
    // sits on that text.
    var body = (document.body && document.body.textContent) || '';
    if (body.indexOf('dsh web authentication required') !== -1) {
      // A remote origin has no IPC back to the shell, so this page signals by
      // navigating to a sentinel path on its own origin; the shell's navigation
      // hook catches that before any request leaves. The raw error is replaced
      // first, so it is never what the user is left looking at.
      try { document.body.textContent = '正在恢复会话…'; } catch (error) {}
      location.replace(location.origin + '/__dsh_shell_handoff_failed__');
    }
  });
})();
"#;

/// Where a page reports a hand-off it could not complete.
///
/// The dsh UI is a remote origin with no IPC back to the shell — deliberately,
/// so the wrapped app never gains the shell's command surface. A page can still
/// navigate, so this path is how it says "I landed on dsh's auth page"; the
/// navigation hook below catches it before any request goes out.
const HANDOFF_FAILED_PATH: &str = "/__dsh_shell_handoff_failed__";

/// Diagnostics relayed from the page. Always printed: a page that fails to run
/// is the one failure the shell cannot otherwise report. See [`DIAG_SCRIPT`].
#[tauri::command]
fn page_diag(stage: String, message: String) {
    eprintln!("[dsh-shell] page {stage}: {message}");
}

/// Recover from a page that reported a failed hand-off.
///
/// One retry with a freshly resolved address — the token lives in the server
/// log, so re-reading it costs nothing — and then the shell page, where the
/// failure and a next step can actually be shown.
fn recover_handoff(app: &AppHandle, shell: &SharedShell) {
    let retry = match shell.lock() {
        Ok(mut guard) => {
            guard.handoff_retries += 1;
            guard.handoff_retries == 1
        }
        Err(_) => false,
    };

    if retry {
        let resolved = shell
            .lock()
            .ok()
            .and_then(|guard| guard.state.port)
            .and_then(server::resolve_ui_url);
        if let Some(parsed) = resolved.and_then(|url| url.parse::<tauri::Url>().ok()) {
            shell_log!("[dsh-shell] hand-off retry -> {parsed}");
            if let Ok(mut guard) = shell.lock() {
                guard.state.url = Some(parsed.to_string());
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.navigate(parsed);
            }
            return;
        }
    }

    if let Ok(mut guard) = shell.lock() {
        guard.state.phase = "failed".into();
        guard.state.error = Some(
            "dsh 界面没有接受这个会话，重试后仍未成功。\n\n\
             可尝试：重启应用；或先在终端运行 dsh web，再打开本应用。"
                .into(),
        );
    }
    let _ = app.emit(EVENT_ERROR, snapshot(shell));
    let home = shell.lock().ok().and_then(|guard| guard.shell_url.clone());
    if let (Some(window), Some(home)) = (app.get_webview_window("main"), home) {
        if let Ok(parsed) = home.parse::<tauri::Url>() {
            let _ = window.navigate(parsed);
        }
    }
}

/// Hand a URL to the desktop's default handler.
///
/// Deliberately not `tauri-plugin-opener`: the whole shell only ever needs
/// "open this http(s) URL", and a plugin would add a dependency, a capability
/// entry and a permission surface for four lines of work.
fn open_external(url: &str) {
    let program = if cfg!(target_os = "windows") {
        "cmd"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(program);
    if cfg!(target_os = "windows") {
        // `start` is a cmd builtin; the empty argument is the window title.
        command.args(["/C", "start", ""]);
    }
    let _ = command.arg(url).spawn();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dismiss_path = app
                .path()
                .app_config_dir()
                .ok()
                .map(|dir| dir.join("dismissed-updates.json"));
            let store = dismiss_path
                .as_ref()
                .map(updates::DismissStore::load)
                .unwrap_or_default();
            shell_log!(
                "[dsh-shell] dismiss store {} -> {:?}",
                dismiss_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<无>".into()),
                store.dismissed,
            );

            // The shell exists before the window because the navigation hook
            // below has to capture it.
            let shell: SharedShell = Arc::new(Mutex::new(Shell {
                state: ShellState {
                    phase: "starting".into(),
                    message: "正在启动…".into(),
                    log_dir: Some(server::log_dir().display().to_string()),
                    ..Default::default()
                },
                store,
                dismiss_path,
                shell_url: None,
                handoff_retries: 0,
            }));

            // The window is built here rather than declared in tauri.conf.json
            // so the navigation hooks below can be attached; there is no way to
            // add them to a config-created window.
            let nav_app = app.handle().clone();
            let nav_shell = shell.clone();
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("DeepSeek Harness")
            .inner_size(1500.0, 940.0)
            .min_inner_size(900.0, 600.0)
            .center()
            .resizable(true)
            .visible(true)
            .initialization_script(DIAG_SCRIPT)
            .background_color(tauri::window::Color(0x14, 0x14, 0x14, 0xff))
            .on_navigation(move |url| {
                // The sentinel a wrapped page uses to report a hand-off it could
                // not complete. Caught here, so the request never leaves.
                if url.path() == HANDOFF_FAILED_PATH {
                    shell_log!("[dsh-shell] the page reported a failed hand-off");
                    recover_handoff(&nav_app, &nav_shell);
                    return false;
                }
                // A link that would replace the app in the same webview opens
                // in the real browser instead.
                let internal = is_internal(url);
                shell_log!(
                    "[dsh-shell] navigate {} -> {}",
                    url,
                    if internal { "allow" } else { "open externally" }
                );
                if internal {
                    return true;
                }
                open_external(url.as_str());
                false
            })
            // `window.open` / `target="_blank"` — the counterpart of Electron's
            // setWindowOpenHandler. New Tauri webviews on Linux must be created
            // with an explicit related-view link, which is not worth the
            // fragility here, so every popup goes to the system browser.
            .on_new_window(|url, _features| {
                open_external(url.as_str());
                tauri::webview::NewWindowResponse::Deny
            })
            .build()?;

            // Read off the window rather than assembled from the platform's
            // scheme, so it stays correct wherever Tauri serves app assets.
            let shell_url = window.url().ok().map(|url| url.to_string());
            if let Ok(mut guard) = shell.lock() {
                guard.shell_url = shell_url;
            }

            app.manage(shell.clone());

            let handle = app.handle().clone();
            std::thread::spawn(move || bootstrap(handle, shell));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            open_dsh,
            page_diag,
            check_updates,
            dismiss_version,
            clear_dismissed,
            dismissed_versions,
            apply_update,
            restart_app,
            dismiss_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the DSH Tauri shell");
}

#[cfg(test)]
mod tests {
    use super::is_internal;

    fn url(value: &str) -> tauri::Url {
        value.parse().expect("test URL must parse")
    }

    #[test]
    fn the_bundled_page_stays_in_the_webview() {
        // Getting this wrong blanks the window before anything else can run,
        // so it is pinned by a test.
        assert!(is_internal(&url("tauri://localhost/index.html")));
        assert!(is_internal(&url("tauri://localhost/")));
        // Windows serves app assets over http://tauri.localhost.
        assert!(is_internal(&url("http://tauri.localhost/index.html")));
    }

    #[test]
    fn the_local_dsh_ui_stays_in_the_webview() {
        assert!(is_internal(&url("http://127.0.0.1:3080/?token=abc")));
        assert!(is_internal(&url("http://localhost:3129/")));
        assert!(is_internal(&url("http://127.0.0.1:3082/xswt-bg/sky.jpg")));
    }

    #[test]
    fn the_handoff_sentinel_is_recognised() {
        use super::{is_internal, HANDOFF_FAILED_PATH};

        let sentinel: tauri::Url = "http://127.0.0.1:3080/__dsh_shell_handoff_failed__"
            .parse()
            .unwrap();
        assert_eq!(sentinel.path(), HANDOFF_FAILED_PATH);
        // The sentinel is a loopback URL, so the link policy would otherwise
        // wave it through; the navigation hook checks it first, which is what
        // keeps the request from ever reaching the server.
        assert!(is_internal(&sentinel));
    }

    #[test]
    fn anything_else_is_a_link_out() {
        assert!(!is_internal(&url("https://github.com/deepseek-ai")));
        assert!(!is_internal(&url("http://example.com/")));
        // A remote host must not slip through on the loopback port band.
        assert!(!is_internal(&url("http://evil.example:3080/")));
        assert!(!is_internal(&url("mailto:a@b.c")));
        assert!(!is_internal(&url("file:///etc/passwd")));
    }
}
