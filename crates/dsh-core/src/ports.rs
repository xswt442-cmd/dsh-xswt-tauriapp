//! Ports: what is on one, whether a server could use one, and which one the
//! user last asked for.
//!
//! Two questions that look alike and are not. A **connect probe** asks whether
//! something is listening right now; it is the cheaper check and the one a scan
//! uses. **`can_bind`** asks whether a server could bind, which is the operation
//! the server is about to perform — a port nothing is listening on can still
//! refuse it, and no timeout makes the wrong question right.

use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// The first port of the band a server can be started on.
pub fn find_free_port() -> Option<u16> {
    (START_PORT..=MAX_PORT).find(|port| can_bind(*port))
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

#[cfg(test)]
mod tests {
    use super::{can_bind, PortMemory};

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
