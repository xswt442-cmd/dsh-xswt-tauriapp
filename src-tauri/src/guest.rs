//! The dsh window, and handing it a session.
//!
//! This is where the harness earns its name. The guest window is dsh's own page
//! and nothing else: no injected script, no DOM reading, no CSS patch, no shell
//! UI drawn over it. Everything here is window and process plumbing.
//!
//! ## Why the guest is created *after* the cookie is written
//!
//! dsh's session cookie is `SameSite=Strict`. A navigation started by a page at
//! another origin is cross-site, so that cookie is withheld and dsh answers with
//! its 401 page — which is exactly what the previous design did, by navigating
//! the *shell page* to dsh, and why the window came up black with "dsh web
//! authentication required" on it.
//!
//! A navigation started by the host has no initiating page at all, and a request
//! with no initiator is not cross-site. So the fix is not to relax dsh's
//! `SameSite`, and not to sniff the 401 and retry: it is to make the guest's
//! **first** navigation a host-initiated one, with the cookie already in the jar.
//! Tauri keeps one web context per data directory, so the cookie written through
//! the bootstrap window's webview is the one the guest finds — and the guest is
//! then built with `WebviewUrl::External(session.url)` and loads it once, already
//! authenticated.

use std::process::Command;
use std::time::{Duration, Instant};

use dsh_xswt_tauriapp_core::{server::Session, zoom};
use tauri::webview::{Cookie, PageLoadEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::shell_log;
use crate::state::{self, Handoff, SharedShell, BOOTSTRAP_LABEL, GUEST_LABEL};

/// The host dsh binds to, and the domain the session cookie has to be filed
/// under.
const LOOPBACK_HOST: &str = "127.0.0.1";
/// How long a cookie write may take to become readable.
const COOKIE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the guest's first load may take before it is called a failure.
const GUEST_LOAD_TIMEOUT: Duration = Duration::from_secs(90);
/// One zoom step. The bounds live in `dsh_core::zoom`, next to the rule for the
/// value to start at and the file it is remembered in.
const ZOOM_STEP: f64 = 1.1;

// ── the session hand-off ───────────────────────────────────────────────────

/// Parse one `Set-Cookie` value into a cookie the webview can store.
///
/// dsh mints a **host-only** cookie, so the header carries no `Domain`
/// attribute. Both runtimes turn a missing domain into an empty one, and a
/// cookie filed under an empty domain is not sent to anything — so the loopback
/// host has to be supplied here, from the address we verified the session on.
fn session_cookie(set_cookie: &str) -> Result<Cookie<'static>, String> {
    let mut cookie = Cookie::parse(set_cookie.to_string())
        .map_err(|error| format!("无法解析 dsh 的 Set-Cookie：{error}"))?;
    if cookie.domain().is_none() {
        cookie.set_domain(LOOPBACK_HOST);
    }
    if cookie.path().is_none() {
        cookie.set_path("/");
    }
    // Anything else — `SameSite=Strict`, `HttpOnly`, `Max-Age` — is the
    // server's decision and is carried through untouched.
    Ok(cookie)
}

/// Whether `name` is present in `window`'s cookie jar for `url`.
///
/// A blocking getter, so this must not run on the main thread: on Windows it
/// deadlocks when it does (wry#583).
fn cookie_present(window: &WebviewWindow, url: &tauri::Url, name: &str) -> bool {
    window
        .cookies_for_url(url.clone())
        .map(|cookies| cookies.iter().any(|cookie| cookie.name() == name))
        .unwrap_or(false)
}

/// Write the session cookie through `window` and wait until it is readable.
///
/// `set_cookie` only posts a message to the event loop, so returning from it
/// does not mean the jar has the cookie yet; re-reading it is both the wait and
/// the confirmation.
///
/// The confirmation is **advisory**. A runtime that cannot report back leaves the
/// cookie written and working, and failing the launch over a diagnostic read
/// would be worse than proceeding — so a timeout is reported as `Ok(false)`.
/// Only a `set_cookie` that refuses outright is an error.
fn write_cookie(
    window: &WebviewWindow,
    url: &tauri::Url,
    cookie: &Cookie<'static>,
) -> Result<bool, String> {
    window
        .set_cookie(cookie.clone())
        .map_err(|error| format!("写入会话 cookie 失败：{error}"))?;

    let deadline = Instant::now() + COOKIE_TIMEOUT;
    while Instant::now() < deadline {
        if cookie_present(window, url, cookie.name()) {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(false)
}

/// Put the session cookie into the cookie jar, before any dsh window exists.
///
/// Runs on the bootstrap worker thread, which is why the reading part is legal.
/// Returns whether the write could be confirmed, and only fails when the cookie
/// could not be parsed or the runtime refused it.
pub fn prime_cookie(app: &AppHandle, session: &Session) -> Result<bool, String> {
    let Some(set_cookie) = session.set_cookie.as_deref() else {
        // The server serves its UI without authentication; there is no session
        // to carry.
        return Ok(true);
    };
    let cookie = session_cookie(set_cookie)?;
    let url: tauri::Url = session
        .url
        .parse()
        .map_err(|error| format!("URL 无效：{error}"))?;
    let window = app
        .get_webview_window(BOOTSTRAP_LABEL)
        .ok_or_else(|| "外壳窗口不存在，无法写入会话。".to_string())?;
    write_cookie(&window, &url, &cookie)
}

/// Confirm the guest's own jar holds the session, and repair it if it does not.
///
/// Only reachable on a runtime that keeps separate cookie jars per webview. When
/// the jar is shared — which is what Tauri does per data directory — this reads
/// back the cookie that `prime_cookie` already wrote and does nothing else.
///
/// The check gets a grace period before it concludes anything: this runs while
/// the guest's first navigation is in flight, and a jar that has not answered
/// yet is not the same as a cookie that is missing. Repairing on a slow read
/// would cost a second full load of dsh's UI, which is the one cost this design
/// is trying not to pay.
fn ensure_cookie(app: &AppHandle, session: &Session) {
    let Some(set_cookie) = session.set_cookie.as_deref() else {
        return;
    };
    let Ok(url) = session.url.parse::<tauri::Url>() else {
        return;
    };
    let Ok(name) = Cookie::parse(set_cookie.to_string()).map(|cookie| cookie.name().to_string())
    else {
        return;
    };
    let Some(window) = app.get_webview_window(GUEST_LABEL) else {
        return;
    };

    let deadline = Instant::now() + COOKIE_TIMEOUT;
    while Instant::now() < deadline {
        // The guest went away (the user closed it, or the app is exiting).
        if app.get_webview_window(GUEST_LABEL).is_none() {
            return;
        }
        if cookie_present(&window, &url, &name) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    shell_log!("[dsh-harness] the guest has no session of its own; writing one and reloading");
    match session_cookie(set_cookie).and_then(|cookie| write_cookie(&window, &url, &cookie)) {
        Ok(true) => {
            // The load already in flight went out without it, so ask again —
            // this time the cookie is in the jar.
            let _ = window.reload();
        }
        Ok(false) => shell_log!("[dsh-harness] the repaired cookie could not be read back"),
        Err(error) => shell_log!("[dsh-harness] could not repair the guest session: {error}"),
    }
}

/// Create the guest window and point it at the prepared session.
///
/// Must run on the main thread: it builds a window. The cookie work has already
/// happened by now (see the module docs).
pub fn spawn(app: &AppHandle, shell: &SharedShell) -> Result<(), String> {
    if app.get_webview_window(GUEST_LABEL).is_some() {
        // An earlier hand-off already built it; nothing to do.
        return Ok(());
    }
    let session = shell
        .lock()
        .map_err(|_| "状态锁不可用".to_string())?
        .session
        .clone()
        .ok_or_else(|| "dsh 服务尚未就绪".to_string())?;
    let url: tauri::Url = session
        .url
        .parse()
        .map_err(|error| format!("URL 无效：{error}"))?;

    let load_shell = shell.clone();
    let window = WebviewWindowBuilder::new(app, GUEST_LABEL, WebviewUrl::External(url))
        .title("DeepSeek Harness")
        .inner_size(1500.0, 940.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .resizable(true)
        // Hidden until the page is actually up, so a failed hand-off surfaces in
        // the bootstrap window instead of as a broken dsh window.
        .visible(false)
        .background_color(tauri::window::Color(0x14, 0x14, 0x14, 0xff))
        .on_navigation(move |url| {
            // A link that would replace dsh in the same webview opens in the
            // real browser instead.
            let internal = is_internal(url);
            shell_log!(
                "[dsh-harness] guest navigate {} -> {}",
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
        // setWindowOpenHandler. New Tauri webviews on Linux must be created with
        // an explicit related-view link, which is not worth the fragility here,
        // so every popup goes to the system browser.
        .on_new_window(|url, _features| {
            open_external(url.as_str());
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                on_loaded(&window, &load_shell);
            } else {
                shell_log!("[dsh-harness] guest loading {}", payload.url());
            }
        })
        .build()
        .map_err(|error| format!("无法创建 dsh 窗口：{error}"))?;

    // The zoom factor lives in the shell rather than in the webview — see
    // `zoom_by` — so a window built after the last `set_zoom` starts at 1.0
    // unless it is told. Zooming the bootstrap page and then handing over is
    // exactly that case, and it would silently lose the setting.
    let zoom = {
        let Ok(mut guard) = shell.lock() else {
            return Err("状态锁不可用".to_string());
        };
        guard.handoff = Handoff::Priming;
        guard.zoom
    };
    let _ = window.set_zoom(zoom);
    shell_log!("[dsh-harness] guest window created for {}", session.url);

    let repair_app = app.clone();
    std::thread::spawn(move || ensure_cookie(&repair_app, &session));

    // A window that never finishes loading would otherwise leave the user on the
    // bootstrap screen with nothing to read.
    let watch_app = app.clone();
    let watch_shell = shell.clone();
    std::thread::spawn(move || watch(&watch_app, &watch_shell));
    Ok(())
}

/// The guest's first finished load is what puts dsh on screen.
fn on_loaded(window: &WebviewWindow, shell: &SharedShell) {
    let priming = {
        let Ok(mut guard) = shell.lock() else { return };
        if guard.handoff != Handoff::Priming {
            return;
        }
        guard.handoff = Handoff::Done;
        guard.state.message = "dsh 已就绪".into();
        true
    };
    if !priming {
        return;
    }
    shell_log!("[dsh-harness] the guest is up; handing the screen over");
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(bootstrap) = window.app_handle().get_webview_window(BOOTSTRAP_LABEL) {
        let _ = bootstrap.hide();
    }
}

/// Fail the launch if the guest never finishes loading.
fn watch(app: &AppHandle, shell: &SharedShell) {
    let deadline = Instant::now() + GUEST_LOAD_TIMEOUT;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        match shell.lock().map(|guard| guard.handoff) {
            // Done or Failed: somebody else has already settled it.
            Ok(Handoff::Priming) => {}
            _ => return,
        }
        if app.get_webview_window(GUEST_LABEL).is_none() {
            return;
        }
        if Instant::now() >= deadline {
            state::fail(
                app,
                shell,
                format!(
                    "dsh 界面在 {} 秒内没有加载完成。\n\n\
                     可尝试：重启应用；或先在终端运行 dsh web，确认界面本身可用。",
                    GUEST_LOAD_TIMEOUT.as_secs()
                ),
            );
            return;
        }
    }
}

/// Tear the guest window down without asking, so a failure can fall back to the
/// bootstrap page.
///
/// `destroy` rather than `close`: `close` would raise `CloseRequested`, which the
/// harness reads as "the user closed dsh" and turns into a quit.
pub fn retire(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(GUEST_LABEL) {
        let _ = window.destroy();
    }
}

// ── window actions, driven by menu / tray / shortcuts ──────────────────────

/// The window a menu item or shortcut should act on: dsh when it exists, else
/// the bootstrap page.
pub fn target_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(GUEST_LABEL)
        .or_else(|| app.get_webview_window(BOOTSTRAP_LABEL))
}

/// Re-show the guest (or the bootstrap page) and focus it.
pub fn show(app: &AppHandle) {
    if let Some(window) = target_window(app) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Reload the current window.
///
/// This is the gesture a browser would provide and a chrome-less window does
/// not have. It matters: dsh writes the content font size and the theme's boot
/// values into the document when the host renders it, and nothing applies them
/// afterwards, so a settings change needs a reload to take effect.
pub fn reload(app: &AppHandle) {
    if let Some(window) = target_window(app) {
        let _ = window.reload();
    }
}

/// Step the zoom factor, clamping it to something usable.
///
/// Driven from Rust rather than the webview's own zoom hotkeys: on macOS and
/// Linux that setting works by *injecting a polyfill into the page*, which is
/// the one thing this harness does not do.
///
/// The factor is remembered as well as applied. On a display whose applications
/// are scaled — Windows at 125%, say — a WSLg session renders every window at
/// scale 1, so the factor is compensating for the machine rather than for the
/// moment; asking for it again on every launch is the whole of that complaint.
fn zoom_by(app: &AppHandle, shell: &SharedShell, factor: f64) {
    let zoom = match shell.lock() {
        Ok(mut guard) => {
            let next = zoom::clamp(guard.zoom * factor).unwrap_or(1.0);
            guard.zoom = next;
            if let Err(error) = guard.zoom_memory.remember(next) {
                shell_log!("[dsh-harness] could not remember the zoom factor: {error}");
            }
            next
        }
        Err(_) => return,
    };
    if let Some(window) = target_window(app) {
        let _ = window.set_zoom(zoom);
    }
}

/// Zoom in one step.
pub fn zoom_in(app: &AppHandle, shell: &SharedShell) {
    zoom_by(app, shell, ZOOM_STEP);
}

/// Zoom out one step.
pub fn zoom_out(app: &AppHandle, shell: &SharedShell) {
    zoom_by(app, shell, 1.0 / ZOOM_STEP);
}

/// Return the zoom factor to 1.0, and remember that too — a reset is a choice.
pub fn zoom_reset(app: &AppHandle, shell: &SharedShell) {
    if let Ok(mut guard) = shell.lock() {
        guard.zoom = 1.0;
        if let Err(error) = guard.zoom_memory.remember(1.0) {
            shell_log!("[dsh-harness] could not remember the zoom factor: {error}");
        }
    }
    if let Some(window) = target_window(app) {
        let _ = window.set_zoom(1.0);
    }
}

/// Whether the WebView inspector is reachable in this build.
///
/// The `devtools` Cargo feature makes the API exist; this decides whether a user
/// gets a menu item for it. A release build ships without it unless asked.
pub fn devtools_available() -> bool {
    cfg!(debug_assertions) || std::env::var_os("DSH_SHELL_DEVTOOLS").is_some()
}

/// Show or hide the WebView inspector for the current window.
pub fn toggle_devtools(app: &AppHandle) {
    if !devtools_available() {
        shell_log!("[dsh-harness] DevTools are off in this build");
        return;
    }
    if let Some(window) = target_window(app) {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
    }
}

// ── link policy ────────────────────────────────────────────────────────────

/// The window's own scheme, plus anything on loopback, stays inside the webview:
/// the bundled bootstrap page and the dsh UI. Everything else is a link out.
pub fn is_internal(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        "http" | "https" => url.host_str().is_some_and(|host| {
            host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host == "::ffff:127.0.0.1"
                || host.ends_with(".localhost")
        }),
        _ => false,
    }
}

/// The bootstrap window's own origins, and nothing else.
///
/// Much narrower than [`is_internal`] on purpose. The bootstrap page is the only
/// window a capability is granted to, so anything allowed to load in it can call
/// the shell's commands — and `is_internal` waves through *any* loopback port,
/// which on a shared machine belongs to whichever program claimed it. The page
/// needs exactly one origin: the assets it was bundled with.
pub fn is_shell_asset(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        // Windows serves the bundled assets over http://tauri.localhost. That one
        // host, not the whole `.localhost` space.
        "http" | "https" => url.host_str().is_some_and(|host| host == "tauri.localhost"),
        _ => false,
    }
}

/// Hand a URL to the desktop's default handler.
///
/// Deliberately not `tauri-plugin-opener`: the whole shell only ever needs
/// "open this http(s) URL", and a plugin would add a dependency, a capability
/// entry and a permission surface for four lines of work.
pub fn open_external(url: &str) {
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

#[cfg(test)]
mod tests {
    use super::{is_internal, is_shell_asset};

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
        // The guest's first load is the hand-off itself: if the policy treated
        // it as a link out, the window would stay blank forever.
        assert!(is_internal(&url("http://127.0.0.1:3080/")));
        assert!(is_internal(&url("http://localhost:3129/")));
        assert!(is_internal(&url("http://127.0.0.1:3082/xswt-bg/sky.jpg")));
    }

    #[test]
    fn the_bootstrap_window_accepts_only_its_own_assets() {
        // The window the capability belongs to. `is_internal` would also allow
        // every loopback port, which is whoever claimed it, not the shell.
        assert!(is_shell_asset(&url("tauri://localhost/index.html")));
        assert!(is_shell_asset(&url("http://tauri.localhost/index.html")));
        assert!(!is_shell_asset(&url("http://127.0.0.1:3080/")));
        assert!(!is_shell_asset(&url("http://localhost:3080/")));
        // One host, not a suffix: another `*.localhost` is somebody else's page.
        assert!(!is_shell_asset(&url("http://not-the-shell.localhost/")));
        assert!(!is_shell_asset(&url("https://github.com/deepseek-ai")));
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
