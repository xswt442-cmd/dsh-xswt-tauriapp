//! dsh's browser handshake, walked in Rust.
//!
//! A dsh web server hands its UI only to a request carrying the token it minted
//! at startup: `GET /?token=…` answers 303 to `/` with a session cookie, and only
//! that follow-up request is served the app. Walking it here is what reduces a
//! server to a [`Session`] — the clean URL plus the cookie that opens it — so the
//! launch token is consumed in Rust and never reaches a page.

use std::time::Duration;

use crate::logs::token_from_log;
use crate::paths::log_dir;
use crate::ports::{probe_port, SCAN_CONNECT_TIMEOUT};

/// How long one HTTP probe may take.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(3);

/// Marker that identifies the DSH UI, so an unrelated server on the port is not
/// mistaken for ours.
pub const UI_MARKER: &str = "DeepSeek Harness";

/// What a dsh web server answers an unauthenticated request with.
///
/// Matched to tell "another program" apart from "a dsh this machine cannot
/// enter": the Windows-side instance seen from WSL, for instance, listens on the
/// same loopback and keeps its token and logs on the other side.
pub(crate) const AUTH_REQUIRED: &str = "dsh web authentication required";

/// One HTTP response, reduced to what the handshake needs.
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    /// HTTP status code, or 0 when the request never completed.
    pub status: u16,
    /// `location` header, if any.
    pub location: Option<String>,
    /// First `set-cookie` header, if any.
    pub set_cookie: Option<String>,
    /// Response body.
    pub body: String,
}

impl HttpResponse {
    /// Whether this response carries the DSH UI.
    pub fn is_ui(&self) -> bool {
        self.status == 200 && self.body.contains(UI_MARKER)
    }
}

/// `GET` a path on a local dsh server, without following redirects so the
/// handshake can be walked by hand.
pub fn http_get(port: u16, path: &str, cookie: Option<&str>) -> HttpResponse {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(HTTP_TIMEOUT)
        .build();
    let mut request = agent.get(&format!("http://127.0.0.1:{port}{path}"));
    if let Some(cookie) = cookie {
        request = request.set("Cookie", cookie);
    }
    match request.call() {
        Ok(response) => HttpResponse {
            status: response.status(),
            location: response.header("location").map(str::to_string),
            set_cookie: response.header("set-cookie").map(str::to_string),
            body: response.into_string().unwrap_or_default(),
        },
        // ureq reports >= 400 as an error; the status still matters here
        // because dsh answers 401 to an unauthenticated request.
        Err(ureq::Error::Status(code, response)) => HttpResponse {
            status: code,
            location: response.header("location").map(str::to_string),
            set_cookie: response.header("set-cookie").map(str::to_string),
            body: response.into_string().unwrap_or_default(),
        },
        Err(_) => HttpResponse::default(),
    }
}

/// A dsh web session, ready to be handed to a webview.
///
/// The point of this type is that the token handshake has *already happened* by
/// the time it exists. A shell writes [`Session::cookie`] into the webview's
/// cookie jar and points it at [`Session::url`] — a clean `/`, with no token in
/// the query string, so nothing on the page can read the launch token back out
/// of `location.search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The UI address to load: `http://127.0.0.1:<port>/`.
    pub url: String,
    /// The `name=value` pair to write into the cookie jar, when the server
    /// authenticates. `None` for a server that serves its UI unauthenticated.
    pub cookie: Option<String>,
    /// The whole `Set-Cookie` header the pair came from, attributes included
    /// (`Path`, `HttpOnly`, `SameSite`, `Max-Age`). Kept so a shell can write
    /// the cookie back with the server's own attributes rather than invented
    /// ones, and so the attributes are visible in a diagnostic dump.
    pub set_cookie: Option<String>,
    /// The port the UI is served on.
    pub port: u16,
}

impl Session {
    /// Whether reaching this UI requires the cookie to be presented.
    pub fn is_authenticated(&self) -> bool {
        self.cookie.is_some()
    }
}

/// The `name=value` pair at the head of a `Set-Cookie` value.
///
/// Attributes follow the first `;`; only the pair is sent back on a request.
pub fn cookie_pair(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The ready session of a dsh web server already listening on `port`, or `None`.
///
/// Walks the token handshake and verifies the result, so a `Some` here means the
/// UI has been fetched successfully at least once and the cookie that did it is
/// in hand. A server on the port that is not dsh answers with something else and
/// is rejected rather than embedded.
pub fn resolve_session(port: u16) -> Option<Session> {
    // The scan bound, not the bind bound: a server that is up accepts at once,
    // and a port with nothing on it must not cost 800ms to rule out.
    if !probe_port(port, SCAN_CONNECT_TIMEOUT) {
        return None;
    }

    // Whatever happened, the address handed out is the clean one; the token is
    // consumed here and never returned.
    let url = format!("http://127.0.0.1:{port}/");
    let unauthenticated = Session {
        url,
        cookie: None,
        set_cookie: None,
        port,
    };

    if let Some(token) = token_from_log(&log_dir(), port) {
        let handshake = http_get(port, &format!("/?token={token}"), None);
        // A server with auth disabled serves the UI straight from the token
        // request; there is no cookie to carry.
        if handshake.is_ui() {
            return Some(unauthenticated);
        }
        if (300..400).contains(&handshake.status) {
            if let Some(set_cookie) = handshake.set_cookie.as_deref() {
                let pair = cookie_pair(set_cookie);
                let path = handshake
                    .location
                    .clone()
                    .unwrap_or_else(|| "/".to_string());
                // Following the redirect with the cookie is what proves the pair
                // is live: a stale token from an earlier server on this port
                // mints a session that does not open the app.
                if !pair.is_empty() && http_get(port, &path, Some(&pair)).is_ui() {
                    return Some(Session {
                        url: unauthenticated.url,
                        cookie: Some(pair),
                        set_cookie: Some(set_cookie.to_string()),
                        port,
                    });
                }
            }
        }
    }

    // A server started without auth still answers a bare GET.
    if http_get(port, "/", None).is_ui() {
        return Some(unauthenticated);
    }
    None
}

/// The usable URL of a dsh web server already listening on `port`, or `None`.
///
/// The session's *clean* URL. Older shells navigated to a token URL so the page
/// could complete the handshake itself; that leaks the launch token into
/// `location.search`, so [`resolve_session`] is the interface to prefer.
pub fn resolve_ui_url(port: u16) -> Option<String> {
    resolve_session(port).map(|session| session.url)
}

#[cfg(test)]
mod tests {
    use super::{cookie_pair, Session};

    #[test]
    fn only_the_pair_is_sent_back_to_the_server() {
        // The attributes are the server's business; echoing them on a request
        // would send `Path=/; HttpOnly; SameSite=Strict` as if it were part of
        // the value.
        assert_eq!(
            cookie_pair(
                "dsh-auth-web=v1.abc.def; Max-Age=2592000; Path=/; HttpOnly; SameSite=Strict"
            ),
            "dsh-auth-web=v1.abc.def"
        );
        // A bare pair, and the degenerate inputs, survive without panicking.
        assert_eq!(cookie_pair("a=b"), "a=b");
        assert_eq!(cookie_pair("a=b; Path=/"), "a=b");
        assert_eq!(cookie_pair(""), "");
        assert_eq!(cookie_pair("; Path=/"), "");
        assert_eq!(cookie_pair("  spaced = value  ; Path=/"), "spaced = value");
    }

    #[test]
    fn a_session_without_a_cookie_is_the_unauthenticated_shape() {
        let session = Session {
            url: "http://127.0.0.1:3080/".to_string(),
            cookie: None,
            set_cookie: None,
            port: 3080,
        };
        assert!(!session.is_authenticated());

        let authenticated = Session {
            cookie: Some("dsh-auth-web=v1.x".to_string()),
            set_cookie: Some("dsh-auth-web=v1.x; Path=/; SameSite=Strict".to_string()),
            ..session
        };
        assert!(authenticated.is_authenticated());
        // The address handed to a page never carries the token, whatever the
        // authentication shape.
        assert_eq!(authenticated.url, "http://127.0.0.1:3080/");
    }
}
