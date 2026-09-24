//! Finding, reusing and starting a local `dsh web` server.
//!
//! The layer that combines the others. It walks the ports the log directory
//! names, asks each whether it is already serving dsh, and answers what a port
//! would do *before* the user commits to it — which is what makes the port a real
//! choice instead of a report. The pieces it builds on are separate on purpose:
//! paths in [`crate::paths`], port arithmetic in [`crate::ports`], the handshake
//! in [`crate::handshake`], the log in [`crate::logs`], process work in
//! [`crate::launch`].

use std::path::Path;
use std::time::Instant;

use crate::handshake::{http_get, resolve_session, Session, AUTH_REQUIRED};
use crate::launch::{spawn_server, wait_for_ui, Launch, SpawnSpec};
use crate::logs::logged_ports;
use crate::paths::log_dir;
use crate::ports::{
    can_bind, find_free_port, probe_port, MAX_PORT, MIN_PORT, SCAN_CONNECT_TIMEOUT, START_PORT,
};

/// The first dsh web session already running, if any.
///
/// Candidates come from [`logged_ports`] rather than the whole 3080–3129 band.
pub fn find_running_session() -> Option<Session> {
    logged_ports(&log_dir())
        .into_iter()
        .find_map(resolve_session)
}

/// The first dsh web UI already listening on the band, if any.
pub fn find_running_url() -> Option<(u16, String)> {
    find_running_session().map(|session| (session.port, session.url))
}

/// What would happen on a port the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortChoice {
    /// A dsh server is already running there and can be entered as it is.
    Reuse(Session),
    /// Nothing is listening; a server would be started there.
    Start,
    /// Something is listening that is not a dsh at all.
    Occupied,
    /// A dsh is listening, but its session cannot be entered from here — its
    /// token and logs belong to another machine or another `$DSH_HOME`. Worth
    /// saying, because "occupied" would read as a bug to whoever started it.
    Foreign,
    /// Below [`MIN_PORT`], where a bind needs privileges.
    TooLow,
}

/// Decide what `port` would mean, without doing any of it.
///
/// This is what lets the dialog answer before the user commits: "will reuse",
/// "will start" and "something else is there" are all knowable up front, and the
/// third one has to be said *before* a 120 second wait rather than after it.
pub fn check_port(port: u16) -> PortChoice {
    if port < MIN_PORT {
        return PortChoice::TooLow;
    }
    if !probe_port(port, SCAN_CONNECT_TIMEOUT) {
        return PortChoice::Start;
    }
    if let Some(session) = resolve_session(port) {
        return PortChoice::Reuse(session);
    }
    // Listening, but not enterable. Ask once more whether what is there at least
    // *is* a dsh, so the two cases can be told apart in the dialog.
    let probe = http_get(port, "/", None);
    if probe.status == 401 && probe.body.contains(AUTH_REQUIRED) {
        PortChoice::Foreign
    } else {
        PortChoice::Occupied
    }
}

/// How many ports the launcher log may offer the dialog at once.
///
/// Every entry costs a connect probe, and a second HTTP probe when something is
/// listening but not enterable, so the list stops here rather than walking every
/// log the machine has ever written. Six is a machine that has used a handful of
/// ports on purpose; what is past it is history.
pub const KNOWN_PORT_LIMIT: usize = 6;

/// A port this machine has run a dsh server on, and what it would do now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPort {
    /// The port.
    pub port: u16,
    /// [`check_port`]'s answer about it.
    pub choice: PortChoice,
}

/// What the ports named by the launcher log would do, newest log first.
///
/// The port is the user's, and today the field takes only a number they have to
/// remember: this is the list discovery already walks, offered as answers instead
/// of as a suggestion. Each entry is asked the question the prompt asks a typed
/// port, so the list cannot offer something the prompt would then refuse — and a
/// port already serving dsh reads as one to enter rather than one to start on.
pub fn known_ports(dir: &Path, limit: usize) -> Vec<KnownPort> {
    logged_ports(dir)
        .into_iter()
        .take(limit)
        .map(|port| KnownPort {
            port,
            choice: check_port(port),
        })
        .collect()
}

/// What the shell should offer before it starts anything.
#[derive(Debug, Clone)]
pub struct Plan {
    /// A server already running that can be entered as it is, if any.
    pub running: Option<Session>,
    /// The port the field offers when the user does not care.
    pub suggested_port: u16,
}

/// Work out what to offer: an already-running server, and a port to suggest.
///
/// `last_chosen` is the port the user typed by hand last time, if there was one.
/// A deliberate choice outranks the first free port of the band, so wanting 3090
/// does not mean asking for it on every launch — but only while it is still
/// free, since a suggestion that cannot be used is worse than the default.
pub fn plan(last_chosen: Option<u16>) -> Result<Plan, String> {
    let running = find_running_session();
    if let Some(session) = &running {
        return Ok(Plan {
            running: running.clone(),
            suggested_port: session.port,
        });
    }
    let suggested_port = last_chosen
        .filter(|port| *port >= MIN_PORT && can_bind(*port))
        .or_else(find_free_port)
        .ok_or_else(|| format!("{START_PORT}–{MAX_PORT} 端口全部被占用，没有可用端口。"))?;
    Ok(Plan {
        running: None,
        suggested_port,
    })
}

/// Reuse a running server or start a new one, blocking until the UI answers.
pub fn launch(timeout_secs: u64) -> Result<Launch, String> {
    if let Some(session) = find_running_session() {
        return Ok(Launch::Reused(session));
    }
    let port = find_free_port()
        .ok_or_else(|| format!("{START_PORT}–{MAX_PORT} 端口全部被占用，没有可用端口。"))?;
    let spec = SpawnSpec::resolve(port, log_dir())?;
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    match wait_for_ui(port, &mut child, timeout_secs) {
        Some(session) => Ok(Launch::Started(session)),
        None => Err(format!(
            "dsh 服务在 {timeout_secs} 秒内未就绪（进程已退出或超时）。\n日志目录：{}",
            spec.log_dir.display()
        )),
    }
}

/// Blocking launch on a worker thread, reporting progress through a callback.
pub fn launch_with_progress<F>(timeout_secs: u64, mut progress: F) -> Result<Launch, String>
where
    F: FnMut(&str),
{
    progress("正在查找已运行的 dsh 服务…");
    if let Some(session) = find_running_session() {
        progress(&format!("已复用端口 {} 上的 dsh 服务", session.port));
        return Ok(Launch::Reused(session));
    }
    progress("未发现运行中的服务，正在启动…");
    let port = find_free_port()
        .ok_or_else(|| format!("{START_PORT}–{MAX_PORT} 端口全部被占用，没有可用端口。"))?;
    let spec = SpawnSpec::resolve(port, log_dir())?;
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    progress(&format!("已在端口 {port} 启动 dsh 服务，等待就绪…"));
    let started = Instant::now();
    if let Some(session) = wait_for_ui(port, &mut child, timeout_secs) {
        progress(&format!("服务已就绪（{} 秒）", started.elapsed().as_secs()));
        Ok(Launch::Started(session))
    } else {
        Err(format!(
            "dsh 服务在 {timeout_secs} 秒内未就绪（进程已退出或超时）。\n日志目录：{}",
            spec.log_dir.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{check_port, known_ports, PortChoice};
    use crate::ports::MIN_PORT;

    #[test]
    fn privileged_ports_are_refused_before_anything_is_probed() {
        // Below MIN_PORT a bind needs root. Answering without touching the
        // network is what makes this testable without a server.
        assert_eq!(check_port(MIN_PORT - 1), PortChoice::TooLow);
        assert_eq!(check_port(80), PortChoice::TooLow);
    }

    #[test]
    fn the_offered_ports_are_the_newest_ones_and_only_a_few() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Written one after another, so the newest log is the last one written —
        // and that is the port whose server is most likely still running.
        for port in [40_001u16, 40_002, 40_003] {
            std::fs::write(dir.path().join(format!("server-{port}.out.log")), "").expect("write");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let offered = known_ports(dir.path(), 2);
        // The cap is what keeps the dialog quick: every entry is probed, and a
        // machine accumulates a log per port it has ever used.
        assert_eq!(
            offered.iter().map(|known| known.port).collect::<Vec<_>>(),
            vec![40_003, 40_002]
        );
        // Each entry carries `check_port`'s own verdict, which is the same answer
        // the prompt gives a typed port — that is what keeps the two from
        // disagreeing about a port the user then confirms.
        assert!(matches!(
            offered[0].choice,
            PortChoice::Start | PortChoice::Occupied | PortChoice::Reuse(_)
        ));

        assert!(known_ports(dir.path(), 0).is_empty());
        // A machine that has never run dsh offers nothing rather than failing.
        assert!(known_ports(std::path::Path::new("/nonexistent/logs"), 6).is_empty());
    }
}
