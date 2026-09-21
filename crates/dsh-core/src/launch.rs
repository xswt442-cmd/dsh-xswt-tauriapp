//! Starting `dsh web` and waiting for its UI to answer.
//!
//! The child is detached deliberately: a shell that dies must not take the server
//! with it, and closing the window must not stop the server.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::handshake::{resolve_session, Session};
use crate::paths::{dsh_home, log_dir, resolve_dsh_bin, resolve_node};
use crate::ports::{can_bind, MIN_PORT};

/// How long we wait for a freshly spawned server to serve its UI.
pub const BOOT_TIMEOUT_SECS: u64 = 120;

/// How a spawned server was started, for the log and for the caller's records.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// `node` executable.
    pub node: PathBuf,
    /// dsh launcher (`lib/bin.js`).
    pub dsh_bin: PathBuf,
    /// Port the server should listen on.
    pub port: u16,
    /// Directory receiving `server-<port>.{out,err}.log`.
    pub log_dir: PathBuf,
}

impl SpawnSpec {
    /// Resolve everything needed to start a server, or explain what is missing.
    pub fn resolve(port: u16, log_dir: PathBuf) -> Result<Self, String> {
        let node = resolve_node().ok_or_else(|| {
            "未找到 node 可执行文件，无法启动 dsh 服务。\n请确认 node 在 PATH 中，或设置 DSH_NODE_BIN。"
                .to_string()
        })?;
        let dsh_bin = resolve_dsh_bin().ok_or_else(|| {
            format!(
                "未找到 dsh 启动器（lib/bin.js）。\n已尝试的路径之一是：\n{}",
                dsh_home()
                    .join("profiles/node_modules/@deepseek-ai/dsh/lib/bin.js")
                    .display()
            )
        })?;
        Ok(Self {
            node,
            dsh_bin,
            port,
            log_dir,
        })
    }

    /// The exact argv used to start the server.
    pub fn argv(&self) -> Vec<String> {
        vec![
            self.dsh_bin.display().to_string(),
            "web".to_string(),
            "--port".to_string(),
            self.port.to_string(),
            "--no-open".to_string(),
        ]
    }
}

/// Start `dsh web` in its own process group with its output appended to the
/// launcher log files, and return the child handle.
///
/// The child is detached so it outlives this shell: a shell that dies must not
/// take the server with it, and closing the window must not stop the server.
/// Both platforms need to be told this explicitly — on Unix a new process
/// group, on Windows `DETACHED_PROCESS`, without which the server dies the
/// moment its parent exits.
pub fn spawn_server(spec: &SpawnSpec) -> std::io::Result<Child> {
    fs::create_dir_all(&spec.log_dir)?;
    let out = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(spec.log_dir.join(format!("server-{}.out.log", spec.port)))?;
    let err = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(spec.log_dir.join(format!("server-{}.err.log", spec.port)))?;

    let mut command = Command::new(&spec.node);
    command
        .args(spec.argv())
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The child owns no console and is not killed with its parent.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    command.spawn()
}

/// Wait until a freshly spawned server serves its UI, or give up.
///
/// Returns `None` when the timeout elapses or the child exits first.
///
/// The deadline is wall-clock, not a count of attempts: one probe can take
/// several seconds (a TCP connect bound plus an HTTP timeout), so counting
/// iterations would let "120 seconds" stretch to many minutes on a host where
/// the server never comes up. The probe also runs before the first sleep, so an
/// already-listening server is not delayed by one interval.
pub fn wait_for_ui(port: u16, child: &mut Child, timeout_secs: u64) -> Option<Session> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(session) = resolve_session(port) {
            return Some(session);
        }
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Outcome of bringing the UI up.
#[derive(Debug, Clone)]
pub enum Launch {
    /// A server was already running; its prepared session.
    Reused(Session),
    /// We started a server; its prepared session.
    Started(Session),
}

impl Launch {
    /// The prepared session, however the server got here.
    pub fn session(&self) -> &Session {
        match self {
            Launch::Reused(session) | Launch::Started(session) => session,
        }
    }

    /// The clean URL the webview should load.
    pub fn url(&self) -> &str {
        &self.session().url
    }

    /// The port the UI is served on.
    pub fn port(&self) -> u16 {
        self.session().port
    }

    /// Whether an existing server was reused rather than started.
    pub fn is_reused(&self) -> bool {
        matches!(self, Launch::Reused(_))
    }
}

/// Reuse the server on `port` if there is one, else start a server there.
///
/// The port is used as given — there is no free-port search behind it, so a port
/// something else owns is an error rather than a silent move to another one. The
/// user asked for this port; quietly ignoring that would be worse than failing.
pub fn start_on_with_progress<F>(port: u16, mut progress: F) -> Result<Launch, String>
where
    F: FnMut(&str),
{
    if port < MIN_PORT {
        return Err(format!(
            "端口 {port} 低于 {MIN_PORT}，普通用户无法绑定。请换一个 ≥ {MIN_PORT} 的端口。"
        ));
    }
    if let Some(session) = resolve_session(port) {
        progress(&format!("已复用端口 {port} 上的 dsh 服务"));
        return Ok(Launch::Reused(session));
    }
    if !can_bind(port) {
        // Not `probe_port`: the question is whether dsh can bind here, and a
        // connect probe that says "nobody is listening" is not an answer to it.
        return Err(format!("端口 {port} 已被其他程序占用。"));
    }
    progress(&format!("正在端口 {port} 启动 dsh 服务…"));
    let spec = SpawnSpec::resolve(port, log_dir())?;
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    let started = Instant::now();
    if let Some(session) = wait_for_ui(port, &mut child, BOOT_TIMEOUT_SECS) {
        progress(&format!("服务已就绪（{} 秒）", started.elapsed().as_secs()));
        Ok(Launch::Started(session))
    } else {
        Err(format!(
            "端口 {port} 上的 dsh 服务在 {BOOT_TIMEOUT_SECS} 秒内未就绪（进程已退出或超时）。\n日志目录：{}",
            spec.log_dir.display()
        ))
    }
}

/// [`start_on_with_progress`] without progress reporting.
pub fn start_on(port: u16) -> Result<Launch, String> {
    start_on_with_progress(port, |_| {})
}
