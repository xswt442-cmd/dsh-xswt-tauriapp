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
//! * The handshake is walked **here**, in Rust, and reduced to a [`Session`]:
//!   the clean URL plus the cookie that opens it. A shell hands the webview the
//!   prepared session rather than a token URL, so the launch token never reaches
//!   a page.
//! * Starting a server must not make the shell its parent in a way that kills
//!   it on exit. Closing the window never stops the server; that is the
//!   instance manager's job.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// First port of the band dsh uses for `web` instances.
pub const START_PORT: u16 = 3080;
/// Last port of the band dsh uses for `web` instances.
pub const MAX_PORT: u16 = 3129;
/// Lowest port the harness will start a server on.
///
/// Below this a bind needs privileges a launched desktop app does not have, so
/// it is refused up front rather than left to fail inside dsh.
pub const MIN_PORT: u16 = 1024;
/// How long a TCP connect may take while *looking for* an existing server.
///
/// Shorter on purpose. A listening loopback socket accepts in microseconds, so
/// this only shortens how long a port that is *not* listening is waited on — and
/// on Windows a closed loopback port has been measured consuming the whole
/// timeout, which turned the 50-port band into a 40 second cold start.
pub const SCAN_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
/// How long one HTTP probe may take.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// How long we wait for a freshly spawned server to serve its UI.
pub const BOOT_TIMEOUT_SECS: u64 = 120;
/// Marker that identifies the DSH UI, so an unrelated server on the port is not
/// mistaken for ours.
pub const UI_MARKER: &str = "DeepSeek Harness";
/// Bytes of log tail scanned for the startup token.
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// What a dsh web server answers an unauthenticated request with.
///
/// Matched to tell "another program" apart from "a dsh this machine cannot
/// enter": the Windows-side instance seen from WSL, for instance, listens on the
/// same loopback and keeps its token and logs on the other side.
const AUTH_REQUIRED: &str = "dsh web authentication required";

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

/// Where the `server-<port>.{out,err}.log` files the token is read back from
/// live.
///
/// Not something dsh owns — it does not write this directory and does not know
/// the name. A launcher writing here is what puts a spawned server's stdout
/// somewhere the token can be recovered from, and other dsh shells read the same
/// files, so the path and the file naming are a convention shared with them
/// rather than this application's to change.
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

/// Whether a server can actually bind `port` on loopback.
///
/// Different question from [`probe_port`], and the one that matters when the
/// answer decides where a server is about to be started. A connect probe asks
/// whether something is *listening* right now; a port it reports free can still
/// refuse the bind — a listener from an earlier attempt may not have settled
/// yet — and the child then dies with `EADDRINUSE` while this shell is waiting
/// for its UI to come up. Binding here is the operation the server is about to
/// perform, so it fails in exactly the cases the server would, and needs no
/// timeout to be right.
pub fn can_bind(port: u16) -> bool {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok()
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

/// How long a log file keeps naming its port as a candidate.
///
/// Long on purpose. Dropping a candidate is not free: if that port is the one
/// holding a running server, the shell stops finding it and starts a second
/// instance instead. So the window has to be longer than the logs a machine
/// accumulates by accident, and only as short as a server's silence is plausible
/// — a dsh web server that has written nothing for three months is not a case
/// worth paying for on every launch.
pub const LOG_PORT_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// Whether a log written at `modified` is still worth reading.
///
/// Pure, so the window can be tested without waiting a quarter or reaching for a
/// file's timestamp. A modification time in the future is a clock that moved,
/// not evidence about the server, so the file is kept rather than dropped.
fn recent_enough(modified: std::time::SystemTime, now: std::time::SystemTime) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age <= LOG_PORT_MAX_AGE,
        Err(_) => true,
    }
}

/// Ports this machine has run a dsh web server on, most recent log first.
///
/// The log file name carries the port (`server-<port>.out.log`), which makes the
/// log directory the list of ports worth probing: a port with no log has no
/// launch token, so [`resolve_session`] could never enter a server there however
/// long it tried. Probing the rest of the band is therefore pure waste — 50
/// connects instead of a handful — and it is also what makes a deliberately
/// unusual port discoverable on the next launch.
///
/// Only logs recent enough to pass [`recent_enough`] count. Nothing ever deletes
/// them, so without a window the list would only grow, at one probe per launch
/// for every port the machine has ever used.
///
/// The one case this gives up is a dsh started by hand in a terminal, whose log
/// went to that terminal instead, *and* which serves its UI without
/// authentication. There is no token to recover for it either way.
pub fn logged_ports(dir: &Path) -> Vec<u16> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let now = std::time::SystemTime::now();
    let mut found: Vec<(std::time::SystemTime, u16)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let port: u16 = name
                .to_str()?
                .strip_prefix("server-")?
                .strip_suffix(".out.log")?
                .parse()
                .ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, port))
        })
        // Logs are appended to and never removed, so without this the candidate
        // list only ever grows and every launch pays for every port the machine
        // has ever used.
        .filter(|(modified, _)| recent_enough(*modified, now))
        .collect();
    // Newest first: the log written most recently belongs to the server most
    // likely to still be running.
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, port)| port).collect()
}

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

/// The first port of the band a server can be started on.
pub fn find_free_port() -> Option<u16> {
    (START_PORT..=MAX_PORT).find(|port| can_bind(*port))
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
    use super::{
        can_bind, check_port, cookie_pair, logged_ports, node_prefix_of, recent_enough,
        PortChoice, PortMemory, Session, LOG_PORT_MAX_AGE, MIN_PORT,
    };
    use std::path::Path;

    #[test]
    fn a_port_something_is_listening_on_cannot_be_bound() {
        // The reason `can_bind` exists rather than a longer connect probe: it is
        // the bind the server is about to attempt, so it is refused in exactly
        // the cases that server would be.
        let listener =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("address").port();
        assert!(!can_bind(port));
    }

    #[test]
    fn a_window_keeps_recent_logs_and_drops_stale_ones() {
        use std::time::{Duration, SystemTime};
        let now = SystemTime::now();
        assert!(recent_enough(now, now));
        // The boundary is inside the window, or a log written exactly at it would
        // flip on the clock rather than on age.
        assert!(recent_enough(now - LOG_PORT_MAX_AGE, now));
        // Just past it: the only thing that stops the candidate list growing.
        assert!(!recent_enough(
            now - LOG_PORT_MAX_AGE - Duration::from_secs(1),
            now
        ));
        // A modification time ahead of the clock is not evidence about a server.
        assert!(recent_enough(now + Duration::from_secs(86_400), now));
    }

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

    #[test]
    fn logged_ports_are_the_candidates_and_nothing_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let write = |name: &str| std::fs::write(dir.path().join(name), "").expect("write");
        write("server-3080.out.log");
        // The newest log belongs to the server most likely to still be running,
        // so it has to be tried first.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write("server-9000.out.log");
        // Not candidates: an error log carries no token, and neither does a file
        // that merely looks similar.
        write("server-3081.err.log");
        write("notes.txt");
        write("server-notaport.out.log");

        assert_eq!(logged_ports(dir.path()), vec![9000, 3080]);
    }

    #[test]
    fn a_missing_log_directory_is_simply_no_candidates() {
        // A machine that has never run dsh has no log directory, which is a
        // normal first launch rather than a failure.
        assert!(logged_ports(Path::new("/nonexistent/dsh-logs")).is_empty());
    }

    #[test]
    fn privileged_ports_are_refused_before_anything_is_probed() {
        // Below MIN_PORT a bind needs root. Answering without touching the
        // network is what makes this testable without a server.
        assert_eq!(check_port(MIN_PORT - 1), PortChoice::TooLow);
        assert_eq!(check_port(80), PortChoice::TooLow);
    }

    #[test]
    fn the_chosen_port_round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("last-port.json");
        let mut memory = PortMemory::load(&path);
        assert_eq!(memory.last, None, "nothing remembered on a first launch");

        memory.remember(9000).expect("persist");
        // Read back the way the next launch would.
        assert_eq!(PortMemory::load(&path).last, Some(9000));

        // A corrupt file must not block startup.
        std::fs::write(&path, "{ not json").expect("write");
        assert_eq!(PortMemory::load(&path).last, None);
    }

    #[test]
    fn an_in_memory_port_memory_never_touches_disk() {
        let mut memory = PortMemory::in_memory();
        memory.remember(3090).expect("remember");
        assert_eq!(memory.last, Some(3090));
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

/// The port the user last chose by hand, persisted as JSON.
///
/// Only an explicit choice is stored. The port the shell picked on its own is
/// not worth remembering — recomputing the first free port is cheaper and stays
/// correct as the machine changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortMemory {
    /// Last port chosen by hand, if there ever was one.
    #[serde(default)]
    pub last: Option<u16>,
    /// Where the value is persisted. Absent for in-memory use.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl PortMemory {
    /// An in-memory store, for tests and for a failed load.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, falling back to "nothing remembered" when the file is
    /// missing or unreadable. A corrupt file must never block startup.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut memory = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        memory.path = Some(path);
        memory
    }

    /// Remember `port` and persist it.
    pub fn remember(&mut self, port: u16) -> Result<(), String> {
        self.last = Some(port);
        self.save()
    }

    /// Persist the value, creating parent directories as needed.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|error| format!("序列化失败：{error}"))?;
        fs::write(path, text).map_err(|error| format!("写入失败：{error}"))
    }
}
