//! Finding, starting and re-entering a local `dsh web` server.
//!
//! Ported from the Electron shell (`dsh-app/main.js`) so the two shells behave
//! identically. The contract that matters:
//!
//! * A dsh web server hands its UI only to a request carrying the token it
//!   minted at startup. A bare `GET /` answers 401, so a server on the port
//!   cannot be recognised — let alone re-entered — without that token, and the
//!   server log is where it lands.
//! * Authentication is a two-step handshake: `GET /?token=<t>` answers 303 to
//!   `/` with a session cookie, and only that follow-up request is served the
//!   app. Walking it also proves the token is live, which rejects a stale one
//!   left behind by an earlier server on the same port.
//! * Starting a server must not make the shell its parent in a way that kills
//!   it on exit. Closing the window never stops the server; that is the
//!   instance manager's job.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// First port of the band dsh uses for `web` instances.
pub const START_PORT: u16 = 3080;
/// Last port of the band dsh uses for `web` instances.
pub const MAX_PORT: u16 = 3129;
/// How long a TCP connect may take before the port counts as closed.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
/// How long one HTTP probe may take.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// How long we wait for a freshly spawned server to serve its UI.
pub const BOOT_TIMEOUT_SECS: u64 = 120;
/// Marker that identifies the DSH UI, so an unrelated server on the port is not
/// mistaken for ours.
pub const UI_MARKER: &str = "DeepSeek Harness";
/// Bytes of log tail scanned for the startup token.
const LOG_TAIL_BYTES: u64 = 256 * 1024;

/// The Harness home: `$DSH_HOME` when it points at an existing directory, else
/// `~/.dsh`.
pub fn dsh_home() -> PathBuf {
    if let Ok(raw) = std::env::var("DSH_HOME") {
        if !raw.is_empty() {
            let path = PathBuf::from(raw);
            if path.is_dir() {
                return path;
            }
        }
    }
    home_dir().join(".dsh")
}

/// The invoking user's home directory.
///
/// A normal Windows process does not get `HOME`, only `USERPROFILE`, so both
/// are consulted. Falling back to the filesystem root keeps the previous
/// behaviour when neither is set.
pub fn home_dir() -> PathBuf {
    let read = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    read("HOME")
        .or_else(|| read("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Where dsh writes the `server-<port>.{out,err}.log` files the token is read
/// back from.
pub fn log_dir() -> PathBuf {
    dsh_home().join("launcher").join("logs")
}

/// File names a `node` executable may have, most likely first.
///
/// Windows PATH entries point at `node.exe`, so probing the bare name there
/// would never match.
pub const NODE_EXE_NAMES: &[&str] = if cfg!(windows) {
    &["node.exe", "node"]
} else {
    &["node"]
};

/// Candidate `node` executables, most specific first.
pub fn node_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("DSH_NODE_BIN") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in NODE_EXE_NAMES {
                out.push(dir.join(name));
            }
        }
    }
    out
}

/// The node prefix that owns an installed dsh launcher.
///
/// A launcher always sits at
/// `<prefix>/lib/node_modules/@deepseek-ai/dsh/lib/bin.js`, so walking up to
/// the `lib/node_modules` boundary names the prefix that dsh is installed
/// under — and therefore the `node` and `npm` that belong to it. Pure path
/// arithmetic, so it is testable without an install.
pub fn node_prefix_of(launcher: &Path) -> Option<PathBuf> {
    let mut dir = launcher.parent()?;
    loop {
        let parent = dir.parent()?;
        let is_node_modules = dir.file_name().is_some_and(|name| name == "node_modules");
        let under_lib = parent.file_name().is_some_and(|name| name == "lib");
        if is_node_modules && under_lib {
            return parent.parent().map(Path::to_path_buf);
        }
        dir = parent;
    }
}

/// The `node` that owns the dsh this process is going to run.
///
/// Preferred over `PATH`, and not merely for tidiness: a desktop launch inherits
/// a minimal `PATH` where the first `node` is often an older system one, and dsh
/// requires a much newer one. Taking that node produces a server that never
/// comes up, or an `npm install -g` into a prefix nobody uses.
pub fn node_for_dsh() -> Option<PathBuf> {
    let launcher = fs::canonicalize(resolve_dsh_bin()?).ok()?;
    let bin_dir = node_prefix_of(&launcher)?.join("bin");
    NODE_EXE_NAMES
        .iter()
        .map(|name| bin_dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The first existing `node` candidate: the one that owns dsh, else `PATH`.
pub fn resolve_node() -> Option<PathBuf> {
    node_for_dsh().or_else(|| node_candidates().into_iter().find(|p| p.is_file()))
}

/// Candidate `dsh` launcher scripts (`lib/bin.js`), most specific first.
///
/// `dsh` on PATH is usually a symlink into a global `node_modules`, so the
/// usable entry point is always `.../@deepseek-ai/dsh/lib/bin.js`.
pub fn dsh_bin_candidates() -> Vec<PathBuf> {
    let suffix = Path::new("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("DSH_BIN") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    out.push(dsh_home().join("profiles").join(&suffix));
    if let Ok(prefix) = std::env::var("npm_config_prefix") {
        if !prefix.is_empty() {
            out.push(PathBuf::from(prefix).join(&suffix));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            // `.../bin/dsh` -> `.../bin/node_modules/...`, which is where npm
            // puts it for a bin-prefix install.
            out.push(dir.join(&suffix));
            // `.../bin/dsh` -> `.../lib/node_modules/...`, the nvm layout.
            if let Some(parent) = dir.parent() {
                out.push(parent.join("lib").join(&suffix));
            }
        }
    }
    out
}

/// The first existing `dsh` launcher.
pub fn resolve_dsh_bin() -> Option<PathBuf> {
    dsh_bin_candidates().into_iter().find(|p| p.is_file())
}

/// The version of the installed dsh, read from the resolved launcher's
/// `package.json` (`…/dsh/lib/bin.js` → `…/dsh/package.json`).
pub fn installed_version() -> Option<String> {
    let bin = resolve_dsh_bin()?;
    let manifest = bin.parent()?.parent()?.join("package.json");
    let text = fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str().map(str::to_string)
}

/// Whether something accepts TCP connections on `port`.
pub fn probe_port(port: u16, timeout: Duration) -> bool {
    let addr = match (std::net::Ipv4Addr::LOCALHOST, port).to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(addr) => addr,
            None => return false,
        },
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// The startup token for `port`, read from the tail of its server log.
///
/// Only the tail matters: the newest entry for the port is the live token.
pub fn token_from_log(dir: &Path, port: u16) -> Option<String> {
    let file = dir.join(format!("server-{port}.out.log"));
    let mut handle = fs::File::open(file).ok()?;
    let len = handle.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let from = len.saturating_sub(LOG_TAIL_BYTES);
    handle.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = String::new();
    handle.read_to_string(&mut buf).ok()?;

    // `dsh web: http://127.0.0.1:<port>/?token=<token>`
    let re = regex::Regex::new(r"dsh web:\s+http://127\.0\.0\.1:(\d+)/\?token=(\S+)").ok()?;
    re.captures_iter(&buf)
        .filter(|cap| cap[1].parse::<u16>() == Ok(port))
        .last()
        .map(|cap| cap[2].to_string())
}

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

/// The usable URL of a dsh web server already listening on `port`, or `None`.
pub fn resolve_ui_url(port: u16) -> Option<String> {
    if !probe_port(port, CONNECT_TIMEOUT) {
        return None;
    }
    if let Some(token) = token_from_log(&log_dir(), port) {
        let token_url = format!("http://127.0.0.1:{port}/?token={token}");
        let handshake = http_get(port, &format!("/?token={token}"), None);
        if handshake.is_ui() {
            return Some(token_url);
        }
        if (300..400).contains(&handshake.status) {
            if let Some(cookie) = handshake.set_cookie.as_deref() {
                let pair = cookie.split(';').next().unwrap_or("");
                let path = handshake
                    .location
                    .clone()
                    .unwrap_or_else(|| "/".to_string());
                if http_get(port, &path, Some(pair)).is_ui() {
                    return Some(token_url);
                }
            }
        }
    }
    // A server started without auth still answers a bare GET.
    if http_get(port, "/", None).is_ui() {
        return Some(format!("http://127.0.0.1:{port}/"));
    }
    None
}

/// The first dsh web UI already listening on the band, if any.
pub fn find_running_url() -> Option<(u16, String)> {
    (START_PORT..=MAX_PORT).find_map(|port| resolve_ui_url(port).map(|url| (port, url)))
}

/// The first port of the band with nothing listening on it.
pub fn find_free_port() -> Option<u16> {
    (START_PORT..=MAX_PORT).find(|port| !probe_port(*port, CONNECT_TIMEOUT))
}

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

#[cfg(test)]
mod tests {
    use super::node_prefix_of;
    use std::path::Path;

    #[test]
    fn the_node_prefix_is_read_off_the_launcher() {
        assert_eq!(
            node_prefix_of(Path::new(
                "/opt/node/lib/node_modules/@deepseek-ai/dsh/lib/bin.js"
            )),
            Some(Path::new("/opt/node").to_path_buf())
        );
        assert_eq!(
            node_prefix_of(Path::new(
                "/home/u/.nvm/versions/node/v24.21.0/lib/node_modules/@deepseek-ai/dsh/lib/bin.js"
            )),
            Some(Path::new("/home/u/.nvm/versions/node/v24.21.0").to_path_buf())
        );
        // A layout without the `lib/node_modules` boundary yields nothing
        // rather than a wrong prefix.
        assert_eq!(node_prefix_of(Path::new("/usr/local/bin/dsh")), None);
    }
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
pub fn wait_for_ui(port: u16, child: &mut Child, timeout_secs: u64) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(url) = resolve_ui_url(port) {
            return Some(url);
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
    /// A server was already running; its URL.
    Reused { port: u16, url: String },
    /// We started a server; its port and URL.
    Started { port: u16, url: String },
}

impl Launch {
    /// The URL the webview should load.
    pub fn url(&self) -> &str {
        match self {
            Launch::Reused { url, .. } | Launch::Started { url, .. } => url,
        }
    }

    /// The port the UI is served on.
    pub fn port(&self) -> u16 {
        match self {
            Launch::Reused { port, .. } | Launch::Started { port, .. } => *port,
        }
    }
}

/// Reuse a running server or start a new one, blocking until the UI answers.
pub fn launch(timeout_secs: u64) -> Result<Launch, String> {
    if let Some((port, url)) = find_running_url() {
        return Ok(Launch::Reused { port, url });
    }
    let port = find_free_port()
        .ok_or_else(|| format!("{START_PORT}–{MAX_PORT} 端口全部被占用，没有可用端口。"))?;
    let spec = SpawnSpec::resolve(port, log_dir())?;
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    match wait_for_ui(port, &mut child, timeout_secs) {
        Some(url) => Ok(Launch::Started { port, url }),
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
    if let Some((port, url)) = find_running_url() {
        progress(&format!("已复用端口 {port} 上的 dsh 服务"));
        return Ok(Launch::Reused { port, url });
    }
    progress("未发现运行中的服务，正在启动…");
    let port = find_free_port()
        .ok_or_else(|| format!("{START_PORT}–{MAX_PORT} 端口全部被占用，没有可用端口。"))?;
    let spec = SpawnSpec::resolve(port, log_dir())?;
    let mut child = spawn_server(&spec).map_err(|error| format!("启动 dsh 服务失败：{error}"))?;
    progress(&format!("已在端口 {port} 启动 dsh 服务，等待就绪…"));
    let started = Instant::now();
    if let Some(url) = wait_for_ui(port, &mut child, timeout_secs) {
        progress(&format!("服务已就绪（{} 秒）", started.elapsed().as_secs()));
        Ok(Launch::Started { port, url })
    } else {
        Err(format!(
            "dsh 服务在 {timeout_secs} 秒内未就绪（进程已退出或超时）。\n日志目录：{}",
            spec.log_dir.display()
        ))
    }
}
