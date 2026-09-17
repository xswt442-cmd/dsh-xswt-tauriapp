//! The startup worker.
//!
//! Discovery, spawning and the registry fetch all block, so they run here rather
//! than in the bootstrap window's thread: the window has to keep painting
//! instead of showing a blank frame for the whole cold start.
//!
//! Startup is two steps rather than one, because the port belongs to the user:
//! [`discover`] reports what is already running and which port it would suggest,
//! the dialog asks, and [`start`] acts on the answer. The server is not spawned
//! until then — which is what makes the port a real choice instead of a report.

use tauri::{AppHandle, Emitter};

use dsh_xswt_tauriapp_core::server;

use crate::shell_log;
use crate::state::{self, Phase, SharedShell, EVENT_CHOOSE, EVENT_READY};
use crate::{guest, update};

/// Find what is running, work out a port to suggest, and offer the choice.
pub fn discover(app: AppHandle, shell: SharedShell) {
    state::set_message(&app, &shell, "正在查找已运行的 dsh 服务…");

    // The registry call is the slow, failure-prone half and the dialog must not
    // wait on it, so it runs alongside discovery and reports by event. That is
    // what keeps "accept the default" as fast as the scan itself, even offline.
    let update_app = app.clone();
    let update_shell = shell.clone();
    std::thread::spawn(move || update::refresh(&update_app, &update_shell));

    let last_chosen = shell.lock().ok().and_then(|guard| guard.port_memory.last);
    let plan = match server::plan(last_chosen) {
        Ok(plan) => plan,
        Err(error) => {
            state::fail(&app, &shell, error);
            return;
        }
    };

    let running_port = plan.running.as_ref().map(|session| session.port);
    if let Ok(mut guard) = shell.lock() {
        guard.state.phase = Phase::Choosing;
        guard.state.default_port = Some(plan.suggested_port);
        guard.state.running_port = running_port;
        guard.state.reused = running_port.is_some();
        guard.state.message = match running_port {
            Some(port) => format!("已发现端口 {port} 上运行中的 dsh 服务"),
            None => format!("将在端口 {} 上启动 dsh 服务", plan.suggested_port),
        };
    }
    shell_log!(
        "[dsh-harness] offering port {} (running: {})",
        plan.suggested_port,
        running_port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "none".into())
    );
    let _ = app.emit(EVENT_CHOOSE, state::snapshot(&shell));
}

/// Start — or adopt — a server on `port`, then prepare the session.
///
/// Runs on its own thread: it can take a server's whole boot, and the dialog
/// that asked for the port is closed by the time it finishes.
pub fn start(app: AppHandle, shell: SharedShell, port: u16) {
    if let Ok(mut guard) = shell.lock() {
        // Out of `Choosing` the moment the choice is made. The page uses this to
        // decide between "offer the dialog" and "wait", so leaving it behind
        // would re-offer the dialog while the server boots.
        guard.state.phase = Phase::Starting;
    }
    state::set_message(&app, &shell, &format!("正在端口 {port} 启动 dsh 服务…"));

    let progress_app = app.clone();
    let progress_shell = shell.clone();
    let launch = server::start_on_with_progress(port, move |message| {
        state::set_message(&progress_app, &progress_shell, message);
    });

    match launch {
        Ok(launch) => hand_over(&app, &shell, launch),
        Err(error) => state::fail(&app, &shell, error),
    }
}

/// Put the session cookie in the jar and announce that a window may be built.
fn hand_over(app: &AppHandle, shell: &SharedShell, launch: server::Launch) {
    let session = launch.session().clone();
    let reused = launch.is_reused();

    // The cookie goes into the jar *here*, before the page can ask for the guest
    // window. That ordering is the whole hand-off: see `guest`'s module docs for
    // why a cookie written after the window exists would be too late.
    match guest::prime_cookie(app, &session) {
        Ok(true) => shell_log!("[dsh-harness] the session cookie is in the jar"),
        Ok(false) => shell_log!(
            "[dsh-harness] the session cookie was written but could not be read back; carrying on"
        ),
        Err(error) => {
            // The dsh UI is behind this cookie. Without it the window would show
            // dsh's own 401 text, which is exactly the failure this design
            // exists to avoid, so it is reported instead.
            state::fail(app, shell, format!("无法把 dsh 会话交给窗口：{error}"));
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
    let _ = app.emit(EVENT_READY, state::snapshot(shell));
}
