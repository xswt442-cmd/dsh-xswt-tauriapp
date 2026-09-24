//! Starting `dsh web` and waiting for its UI to answer.
//!
//! The child is detached deliberately: a shell that dies must not take the server
//! with it, and closing the window must not stop the server.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::handshake::{resolve_session, Session};
use crate::logs;
use crate::paths::{dsh_home, installed_version, log_dir, resolve_dsh_bin, resolve_node};
use crate::ports::{can_bind, probe_port, MIN_PORT, SCAN_CONNECT_TIMEOUT};

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

    /// The stdout log this server's output is appended to.
    pub fn out_path(&self) -> PathBuf {
        logs::out_path(&self.log_dir, self.port)
    }

    /// The stderr log, which is the one a failed start quotes back.
    pub fn err_path(&self) -> PathBuf {
        logs::err_path(&self.log_dir, self.port)
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
        .open(spec.out_path())?;
    let err = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(spec.err_path())?;

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

/// Whether the server process we spawned is still there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildState {
    /// Still running, so the port is still taken.
    Running,
    /// Gone, with the exit code when the platform gave one.
    Exited(Option<i32>),
}

/// What a wait that produced no session can be read from.
///
/// Three separate answers, because they used to be collapsed into one sentence
/// and that sentence was wrong: 0.0.12's handshake regression had a server
/// listening and serving while the dialog said the process had probably died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootEvidence {
    /// The child's state when the wait gave up.
    pub child: ChildState,
    /// Whether anything is listening on the port.
    pub listening: bool,
    /// Whether *this* run wrote a startup token.
    pub token: bool,
}

/// What actually went wrong, as far as it can be told from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootFailure {
    /// The child exited before its UI answered.
    Exited(Option<i32>),
    /// The child is alive and nothing is listening: it never bound the port.
    NotListening,
    /// Something is listening, but this run wrote no token to start a handshake.
    NoToken,
    /// A token was written and the handshake still did not complete.
    HandshakeFailed,
}

/// Read the failure from the evidence. Pure, so every arm is pinned by a test.
pub fn boot_failure(evidence: BootEvidence) -> BootFailure {
    match evidence.child {
        ChildState::Exited(code) => BootFailure::Exited(code),
        ChildState::Running => match (evidence.listening, evidence.token) {
            (false, _) => BootFailure::NotListening,
            (true, false) => BootFailure::NoToken,
            (true, true) => BootFailure::HandshakeFailed,
        },
    }
}

impl BootFailure {
    /// What to tell the user, given what to look at next.
    ///
    /// Each arm says only what was observed and what follows from it. The dsh
    /// version is named in the one case whose cause may be on this side of the
    /// fence, and the stderr quoted is from this run — never from the log's tail,
    /// which may be the previous run's dying words on the same port.
    pub fn describe(
        &self,
        port: u16,
        timeout_secs: u64,
        log_dir: &Path,
        dsh_version: Option<&str>,
        stderr: Option<&str>,
    ) -> String {
        let mut message = match self {
            BootFailure::Exited(None) => {
                format!("端口 {port} 上的 dsh 进程退出了，界面没有起来。")
            }
            BootFailure::Exited(Some(code)) => {
                format!("端口 {port} 上的 dsh 进程退出了（退出码 {code}），界面没有起来。")
            }
            BootFailure::NotListening => format!(
                "dsh 进程还在，但 {timeout_secs} 秒内没有在端口 {port} 上监听；端口仍被它占着。"
            ),
            BootFailure::NoToken => format!(
                "端口 {port} 上有服务在监听，但这次启动没有写下 token，握手无从开始；\
                 端口仍被占着（若它是别的环境启动的，本机也进不去）。"
            ),
            BootFailure::HandshakeFailed => {
                let version = match dsh_version {
                    Some(version) => format!("本机 dsh 版本 {version}"),
                    None => "本机 dsh 版本未知".to_string(),
                };
                format!(
                    "端口 {port} 上的服务在监听、token 也拿到了，但握手没有完成：\
                     换不到会话 cookie，或者换到的 cookie 打不开界面。{version}——\
                     若它比本壳新，很可能是它改了握手方式。"
                )
            }
        };
        if let Some(stderr) = stderr {
            message.push_str(&format!("\n它这次写到 stderr 的最后几行：\n{stderr}"));
        }
        message.push_str(&format!("\n日志目录：{}", log_dir.display()));
        message
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
    // Where this run's output begins. Nothing rotates these logs and they are
    // only appended to, so a tail read after the fact may belong to the previous
    // run on the same port: the offset is what makes the evidence this run's.
    let out_from = fs::metadata(spec.out_path())
        .map(|meta| meta.len())
        .unwrap_or(0);
    let err_from = fs::metadata(spec.err_path())
        .map(|meta| meta.len())
        .unwrap_or(0);
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    let started = Instant::now();
    if let Some(session) = wait_for_ui(port, &mut child, BOOT_TIMEOUT_SECS) {
        progress(&format!("服务已就绪（{} 秒）", started.elapsed().as_secs()));
        Ok(Launch::Started(session))
    } else {
        let evidence = BootEvidence {
            child: match child.try_wait() {
                Ok(Some(status)) => ChildState::Exited(status.code()),
                // A child that cannot be asked is reported as still there: that
                // makes the message say the port is taken, which is the claim
                // that cannot mislead.
                Ok(None) | Err(_) => ChildState::Running,
            },
            listening: probe_port(port, SCAN_CONNECT_TIMEOUT),
            token: logs::token_since(&spec.log_dir, port, out_from).is_some(),
        };
        Err(boot_failure(evidence).describe(
            port,
            BOOT_TIMEOUT_SECS,
            &spec.log_dir,
            installed_version().as_deref(),
            logs::stderr_since(&spec.log_dir, port, err_from).as_deref(),
        ))
    }
}

/// [`start_on_with_progress`] without progress reporting.
pub fn start_on(port: u16) -> Result<Launch, String> {
    start_on_with_progress(port, |_| {})
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{boot_failure, BootEvidence, BootFailure, ChildState};

    fn running(listening: bool, token: bool) -> BootEvidence {
        BootEvidence {
            child: ChildState::Running,
            listening,
            token,
        }
    }

    #[test]
    fn each_failed_boot_is_read_from_its_own_evidence() {
        // The four cases one sentence used to cover, and the reason they had to
        // be pulled apart: only two of them are about a process that died.
        assert_eq!(
            boot_failure(BootEvidence {
                child: ChildState::Exited(Some(1)),
                listening: false,
                token: false,
            }),
            BootFailure::Exited(Some(1))
        );
        // A process killed by a signal has no code, which is not "still running".
        assert_eq!(
            boot_failure(BootEvidence {
                child: ChildState::Exited(None),
                listening: true,
                token: true,
            }),
            BootFailure::Exited(None)
        );
        assert_eq!(
            boot_failure(running(false, false)),
            BootFailure::NotListening
        );
        assert_eq!(boot_failure(running(true, false)), BootFailure::NoToken);
        assert_eq!(
            boot_failure(running(true, true)),
            BootFailure::HandshakeFailed
        );
    }

    #[test]
    fn a_handshake_failure_names_the_version_and_never_a_dead_process() {
        // 0.0.12 in one assertion: the server was up, the token was in hand, and
        // the message said the process had probably exited. It must not be able
        // to say that again.
        let described = BootFailure::HandshakeFailed.describe(
            3600,
            120,
            Path::new("/home/u/.dsh/launcher/logs"),
            Some("0.1.7-rc.1"),
            None,
        );
        assert!(described.contains("token 也拿到了"), "{described}");
        assert!(described.contains("0.1.7-rc.1"), "{described}");
        assert!(described.contains("改了握手方式"), "{described}");
        assert!(!described.contains("进程退出"), "{described}");
        assert!(described.contains("日志目录：/home/u/.dsh/launcher/logs"));
    }

    #[test]
    fn an_exited_child_is_reported_with_its_code_and_this_run_s_stderr() {
        let described = BootFailure::Exited(Some(2)).describe(
            3080,
            120,
            Path::new("/logs"),
            Some("0.1.7-rc.1"),
            Some("Error: plugin tree failed to load"),
        );
        assert!(described.contains("退出码 2"), "{described}");
        assert!(
            described.contains("plugin tree failed to load"),
            "{described}"
        );
        // The version is not the point when the process is gone.
        assert!(!described.contains("0.1.7-rc.1"), "{described}");
    }

    #[test]
    fn a_process_that_is_still_there_is_said_to_hold_the_port() {
        // The other half of the old message's problem: a server that never bound,
        // or never wrote a token, leaves a process behind, and the next launch on
        // that port will find it. Saying so is the difference between a bug and a
        // user who waits for a port to come free.
        for failure in [BootFailure::NotListening, BootFailure::NoToken] {
            let described = failure.describe(3080, 120, Path::new("/logs"), None, None);
            assert!(described.contains("仍被"), "{described}");
            assert!(!described.contains("进程退出了"), "{described}");
        }
    }

    #[test]
    fn a_failure_without_a_tail_is_still_a_sentence() {
        let described =
            BootFailure::NotListening.describe(3080, 120, Path::new("/logs"), None, None);
        assert!(!described.contains("stderr"), "{described}");
        assert_eq!(described.lines().count(), 2, "{described}");
    }
}
