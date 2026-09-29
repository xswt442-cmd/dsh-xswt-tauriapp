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
//! Everything that is not admitted is handed to the desktop's default handler —
//! but only if [`open_external`] can tell that is safe to ask of it, which a
//! `file:` URL cannot.

use std::process::Command;

use crate::shell_log;

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

/// Whether the desktop's opener may be handed `url` at all.
///
/// `file:` is the reason this list exists. The openers do not argue about what
/// they are handed: `explorer` and `open` will run an executable named on the
/// command line, and dsh writes files to disk, so a link naming one is a launch
/// rather than a browse — and a model's output is where that link would come
/// from. An unrecognised scheme is no better, because then the operating system
/// picks the handler with nothing from here to constrain it.
///
/// `mailto:` is allowed because a dsh answer can legitimately carry one and no
/// mail handler resolves to a path this shell would run. A refused link is
/// dropped rather than opened somewhere unexpected, so the cost of a scheme
/// missing here is one dead link, while the cost of one present by mistake is
/// the whole machine.
///
/// Text that does not parse as a URL is refused with the rest: the callers hand
/// this strings from a webview and from a release feed, and neither owes a
/// well-formed address.
pub fn is_openable(url: &str) -> bool {
    match url.parse::<tauri::Url>() {
        Ok(parsed) => matches!(parsed.scheme(), "http" | "https" | "mailto"),
        Err(_) => false,
    }
}

/// Hand a URL to the desktop's default handler, if its scheme says that is safe.
///
/// The gate lives here rather than at the call sites on purpose: every one of
/// them is a webview callback fed by a page this shell does not own, and a
/// choke point cannot be forgotten the way a rule each caller must remember can.
///
/// Deliberately not `tauri-plugin-opener`: the whole shell only ever needs
/// "open this in the desktop's own handler", and a plugin would add a
/// dependency, a capability entry and a permission surface for a few lines of
/// work.
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
    if !is_openable(url) {
        // Said out loud rather than swallowed: a link that does nothing reads as
        // a broken shell, and this is the only place that knows why.
        shell_log!("[dsh-harness] refused to open this link: {url}");
        return;
    }
    let Ok(mut child) = Command::new(opener_for(std::env::consts::OS))
        .arg(url)
        .spawn()
    else {
        return;
    };
    // Waited for on a thread rather than here, because the caller is the webview's
    // navigation callback and `xdg-open` may not answer until the browser is up.
    // Nothing is learnt from the exit code — a link this refuses is refused above
    // — but an unwaited child is a zombie per clicked link on Unix.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// Hand a verified installer to the desktop's opener, and say whether it went.
///
/// The difference from [`open_external`] is that this one gets an answer — on
/// every platform. `xdg-open` on a machine with no handler for a `.deb` (a bare
/// WSL image is the usual one) runs, exits non-zero and opens nothing; `open`
/// reports its failures as an exit code too; and on Windows `ShellExecuteW`
/// answers with an error code where `explorer` answered with nothing at all. A
/// caller that cannot tell "handed over" from "nothing happened" reports an
/// update as done on a machine where nothing happened, which is what this
/// function exists to prevent.
pub fn open_installer(path: &std::path::Path) -> Result<(), String> {
    hand_over(path)
}

/// Windows' half of [`open_installer`]: the shell API, not a process.
///
/// `explorer` is why this is not a `Command`. It is a single-instance program
/// that forwards the request to the already-running shell and exits, so its exit
/// code is not a statement about the file and waiting on it would invent
/// failures. `ShellExecuteW` is the API `explorer` itself calls, and it *does*
/// answer: above 32 means the shell took the request, below it is one of the
/// `SE_ERR_*` codes. That is the difference between an installer that started and
/// a machine that could not open the file at all.
#[cfg(windows)]
fn hand_over(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    /// `SW_SHOWNORMAL`: the installer decides its own window.
    const SW_SHOWNORMAL: i32 = 1;
    /// What the shell API returns when it worked: anything above this.
    const SHELL_EXECUTE_OK: isize = 32;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> *mut std::ffi::c_void;
    }

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both wide strings are NUL-terminated and outlive the call, and the
    // three arguments this does not use are null, which the API documents as
    // "not given".
    let answer = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if answer > SHELL_EXECUTE_OK {
        Ok(())
    } else {
        Err(shell_execute_error(answer))
    }
}

/// The other two platforms' half: the opener's exit code is the answer.
#[cfg(not(windows))]
fn hand_over(path: &std::path::Path) -> Result<(), String> {
    let program = opener_for(std::env::consts::OS);
    match Command::new(program).arg(path).output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!("{program} 退出码 {:?}", output.status.code())),
        Err(error) => Err(format!("无法运行 {program}：{error}")),
    }
}

/// What a `ShellExecuteW` failure code means, in the words the dialog uses.
///
/// Compiled for the tests as well as for Windows: the codes are the part worth
/// pinning, and the machine that runs the suite is not the machine that has them.
#[cfg(any(windows, test))]
fn shell_execute_error(code: isize) -> String {
    match code {
        // `SE_ERR_FNF` and `SE_ERR_PNF`: the installer was moved or deleted
        // between the download and the hand-over.
        2 | 3 => "安装包不在了".to_string(),
        // `SE_ERR_ACCESSDENIED`.
        5 => "系统拒绝了这次打开（权限不足）".to_string(),
        // `SE_ERR_NOASSOC`: nothing on this machine is registered for the file.
        31 => "系统里没有能打开它的程序".to_string(),
        other => format!("系统外壳返回 {other}"),
    }
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
    use super::{is_openable, is_session, is_shell_asset, opener_for, shell_execute_error};

    fn url(value: &str) -> tauri::Url {
        value.parse().expect("test URL must parse")
    }

    #[test]
    fn only_the_session_itself_stays_in_the_webview() {
        // The guest's first load is the hand-off itself: if the policy treated it
        // as a link out, the window would stay blank forever. The cases below it
        // are the ones that cannot be entered, so they open externally —
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
    fn a_link_out_never_names_a_local_file() {
        // `explorer` and `open` run what they are handed, so a link naming an
        // executable is a launch — and dsh writes files to disk, so the path can
        // come straight out of a model's answer. The rest are the schemes where
        // the operating system, not this shell, would choose the handler.
        assert!(is_openable("https://github.com/deepseek-ai"));
        assert!(is_openable("http://127.0.0.1:3080/help"));
        assert!(is_openable("mailto:a@b.c"));
        assert!(!is_openable("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_openable("file:///etc/passwd"));
        assert!(!is_openable("data:text/html,<script>alert(1)</script>"));
        assert!(!is_openable("javascript:alert(1)"));
        assert!(!is_openable("about:blank"));
        // A UNC share is the quiet one: it opens in Explorer, and reaching a
        // remote share leaks the credentials the platform already holds.
        assert!(!is_openable("file://server/share/tool.exe"));
        // Not a URL at all goes out the same door.
        assert!(!is_openable("not a url at all"));
        assert!(!is_openable(""));
    }

    #[test]
    fn a_link_is_never_opened_through_a_command_interpreter() {
        // `cmd /C start` re-parses its argument, so a `&` inside a URL ends the
        // command and whatever follows it runs; `%VAR%` is expanded even inside
        // quotes. A URL is not a token, and the opener has to be one that takes
        // it whole — which rules out any command interpreter.
        assert_eq!(opener_for("windows"), "explorer");
        assert_eq!(opener_for("macos"), "open");
        assert_eq!(opener_for("linux"), "xdg-open");
    }

    #[test]
    fn a_windows_failure_is_read_from_the_shell_api() {
        // The codes `ShellExecuteW` returns below 32. Only one of these is worth
        // its own words — "nothing on this machine opens this file" is the one a
        // user can act on — but none of them may be reported as a success, which
        // is what `explorer`'s silence amounted to.
        assert!(shell_execute_error(2).contains("不在了"));
        assert!(shell_execute_error(3).contains("不在了"));
        assert!(shell_execute_error(5).contains("权限"));
        assert_eq!(shell_execute_error(31), "系统里没有能打开它的程序");
        // An unknown code is still reported rather than swallowed.
        assert!(shell_execute_error(1155).contains("1155"));
    }
}
