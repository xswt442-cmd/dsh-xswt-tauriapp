//! Handing dsh's session cookie to the window that will show dsh.
//!
//! dsh's session cookie is `SameSite=Strict`, so a navigation started by a page
//! at another origin is cross-site and the cookie is withheld — dsh answers with
//! its 401 page, which is what the previous design did by navigating the *shell
//! page* to dsh. A navigation started by the host has no initiating page, and a
//! request with no initiator is not cross-site. So the cookie goes into the jar
//! before any dsh window exists: Tauri keeps one web context per data directory,
//! so the cookie written through the bootstrap window's webview is the one the
//! guest finds.

use std::time::{Duration, Instant};

use dsh_xswt_tauriapp_core::handshake::Session;
use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::shell_log;
use crate::state::{BOOTSTRAP_LABEL, GUEST_LABEL};

/// The host dsh binds to, and the domain the session cookie has to be filed
/// under.
const LOOPBACK_HOST: &str = "127.0.0.1";

/// How long a cookie write may take to become readable.
const COOKIE_TIMEOUT: Duration = Duration::from_secs(5);

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
pub(super) fn ensure_cookie(app: &AppHandle, session: &Session) {
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
