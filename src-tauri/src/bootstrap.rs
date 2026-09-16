//! The startup worker.
//!
//! Discovery, spawning and the registry fetch all block, so they run here rather
//! than in the bootstrap window's thread: the window has to keep painting
//! instead of showing a blank frame for the whole cold start.

use tauri::{AppHandle, Emitter};

use dsh_xswt_tauriapp_core::server;

use crate::shell_log;
use crate::state::{self, Phase, SharedShell, EVENT_READY};
use crate::{guest, update};

/// Bring the UI up, hand the session to the cookie jar, then check for updates.
pub fn run(app: AppHandle, shell: SharedShell) {
    state::set_message(&app, &shell, "正在查找已运行的 dsh 服务…");

    let progress_app = app.clone();
    let progress_shell = shell.clone();
    let launch = server::launch_with_progress(server::BOOT_TIMEOUT_SECS, move |message| {
        state::set_message(&progress_app, &progress_shell, message);
    });

    let launch = match launch {
        Ok(launch) => launch,
        Err(error) => {
            state::fail(&app, &shell, error);
            return;
        }
    };

    let session = launch.session().clone();
    let reused = launch.is_reused();

    // The cookie goes into the jar *here*, before the page can ask for the guest
    // window. That ordering is the whole hand-off: see `guest`'s module docs for
    // why a cookie written after the window exists would be too late.
    match guest::prime_cookie(&app, &session) {
        Ok(true) => shell_log!("[dsh-harness] the session cookie is in the jar"),
        Ok(false) => shell_log!(
            "[dsh-harness] the session cookie was written but could not be read back; carrying on"
        ),
        Err(error) => {
            // The dsh UI is behind this cookie. Without it the window would show
            // dsh's own 401 text, which is exactly the failure this design
            // exists to avoid, so it is reported instead.
            state::fail(&app, &shell, format!("无法把 dsh 会话交给窗口：{error}"));
            return;
        }
    }

    if let Ok(mut guard) = shell.lock() {
        guard.state.phase = Phase::Ready;
        guard.state.url = Some(session.url.clone());
        guard.state.port = Some(session.port);
        guard.state.reused = reused;
        guard.state.message = if reused {
            "已复用运行中的 dsh 服务".into()
        } else {
            "服务已就绪".into()
        };
        guard.session = Some(session);
    }
    let _ = app.emit(EVENT_READY, state::snapshot(&shell));

    update::refresh(&app, &shell);
}
