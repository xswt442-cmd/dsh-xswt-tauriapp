//! Where a navigation may go, and how a link leaves.
//!
//! Two windows, two policies, and the difference is load-bearing:
//!
//! * `bootstrap` carries the only capability, so anything allowed to load in it
//!   can call the shell's commands. It admits its own bundled assets and nothing
//!   else.
//! * `dsh` admits the session it was handed and nothing else — not even another
//!   loopback port, because dsh names its session cookie after the whole
//!   authority it was minted for.
//!
//! Everything that is not admitted is handed to the desktop's default handler.

use std::process::Command;

/// Whether `url` is the session this window was handed, and nothing else.
///
/// Narrower than "anything on loopback", deliberately. dsh names its session
/// cookie after the whole authority it was minted for — `dsh-auth-<hash>` over
/// `host:port` — so another loopback port is a different session, and so is the
/// same port spelled `localhost` instead of `127.0.0.1`. Either one would land
/// the window on dsh's 401 page with no way forward, which reads as a broken
/// shell; handing it to the browser instead is the honest answer, and that is
/// what a `false` here means to the caller.
///
/// The webview's own scheme is the exception. Nothing in the guest navigates to
/// `tauri://`, but treating one as a link out would be worse than allowing it.
pub fn is_session(url: &tauri::Url, session: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        "http" | "https" => {
            url.scheme() == session.scheme()
                && url.host_str() == session.host_str()
                && url.port_or_known_default() == session.port_or_known_default()
        }
        _ => false,
    }
}

/// The bootstrap window's own origins, and nothing else.
///
/// Much narrower than [`is_session`] on purpose. The bootstrap page is the only
/// window a capability is granted to, so anything allowed to load in it can call
/// the shell's commands — and even the dsh session is a page this shell does not
/// own. This window needs exactly one origin: the assets it was bundled with.
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
/// entry and a permission surface for a few lines of work.
///
/// Windows gets `explorer` rather than `cmd /C start`. `cmd` re-parses the
/// string it is handed, and a URL is not a token: a `&` in a query string ends
/// the command, so `http://host/?a=1&<anything>` runs `<anything>` in the user's
/// own session — reachable from any link the dsh page renders, which is every
/// link in a model's output or a plugin's client code. Quoting the URL closes
/// that hole, but cmd expands `%VAR%` even inside quotes, so a link could still
/// name an environment variable and have its value handed to the browser.
/// `explorer` takes the URL as its own argument and neither parses nor expands
/// it; `open` and `xdg-open` already work that way.
pub fn open_external(url: &str) {
    let _ = Command::new(opener_for(std::env::consts::OS))
        .arg(url)
        .spawn();
}

/// The program that hands a URL to the desktop's default handler.
///
/// Split out so the platform's answer is readable in one place, and pinnable —
/// see [`open_external`] for why the Windows one is not the command interpreter.
fn opener_for(os: &str) -> &'static str {
    if os == "windows" {
        "explorer"
    } else if os == "macos" {
        "open"
    } else {
        "xdg-open"
    }
}

#[cfg(test)]
mod tests {
    use super::{is_session, is_shell_asset, opener_for};

    fn url(value: &str) -> tauri::Url {
        value.parse().expect("test URL must parse")
    }

    #[test]
    fn only_the_session_itself_stays_in_the_webview() {
        // The guest's first load is the hand-off itself: if the policy treated it
        // as a link out, the window would stay blank forever. The cases below it
        // are the ones that used to be waved through and cannot be entered —
        // dsh names its cookie after the authority it was minted for.
        let session = url("http://127.0.0.1:3080/");

        assert!(is_session(&url("http://127.0.0.1:3080/"), &session));
        // The page's own sub-navigations and the assets it serves itself.
        assert!(is_session(
            &url("http://127.0.0.1:3080/xswt-bg/sky.jpg"),
            &session
        ));

        // Another loopback port is another dsh, or somebody else's server.
        assert!(!is_session(&url("http://127.0.0.1:3081/"), &session));
        assert!(!is_session(&url("http://127.0.0.1:3129/"), &session));
        // The same port spelled differently is a different cookie name, so the
        // window could not log in to it either.
        assert!(!is_session(&url("http://localhost:3080/"), &session));
        assert!(!is_session(&url("https://127.0.0.1:3080/"), &session));

        // Everything else is a link out.
        assert!(!is_session(
            &url("https://github.com/deepseek-ai"),
            &session
        ));
        assert!(!is_session(&url("http://example.com/"), &session));
        assert!(!is_session(&url("http://evil.example:3080/"), &session));
        assert!(!is_session(&url("mailto:a@b.c"), &session));
        assert!(!is_session(&url("file:///etc/passwd"), &session));

        // The webview's own scheme is never a link out.
        assert!(is_session(&url("tauri://localhost/index.html"), &session));
        assert!(is_session(&url("tauri://localhost/"), &session));
    }

    #[test]
    fn the_bootstrap_window_accepts_only_its_own_assets() {
        // The window the capability belongs to. Even the dsh session is a page
        // this shell does not own, so this window gets one origin and no more.
        assert!(is_shell_asset(&url("tauri://localhost/index.html")));
        assert!(is_shell_asset(&url("http://tauri.localhost/index.html")));
        assert!(!is_shell_asset(&url("http://127.0.0.1:3080/")));
        assert!(!is_shell_asset(&url("http://localhost:3080/")));
        // One host, not a suffix: another `*.localhost` is somebody else's page.
        assert!(!is_shell_asset(&url("http://not-the-shell.localhost/")));
        assert!(!is_shell_asset(&url("https://github.com/deepseek-ai")));
    }

    #[test]
    fn a_link_is_never_opened_through_a_command_interpreter() {
        // `cmd /C start` re-parses its argument, so a `&` inside a URL ends the
        // command and whatever follows it runs; `%VAR%` is expanded even inside
        // quotes. A URL is not a token, and the opener has to be one that takes
        // it whole — which rules out the interpreter this used to go through.
        assert_eq!(opener_for("windows"), "explorer");
        assert_eq!(opener_for("macos"), "open");
        assert_eq!(opener_for("linux"), "xdg-open");
    }
}
