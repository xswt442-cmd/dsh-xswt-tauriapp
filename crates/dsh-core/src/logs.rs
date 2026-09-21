//! The launcher log: where a spawned server's stdout goes, and where its startup
//! token is read back from.
//!
//! This directory is not dsh's — dsh does not write it and does not know the
//! name. It is an external convention: wherever a launcher sends a spawned
//! server's output is where its token can be recovered from, and the file names
//! are part of that convention rather than this application's to change.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

/// Bytes of log tail scanned for the startup token.
const LOG_TAIL_BYTES: u64 = 256 * 1024;

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
    let mut buf = Vec::new();
    handle.read_to_end(&mut buf).ok()?;
    // Bytes rather than `read_to_string`: the window starts at `len - 256 KiB`,
    // an arbitrary byte offset, so it can begin inside a multi-byte character —
    // and a decode that fails there answers `None` for a token sitting in plain
    // sight below it. The replacement characters it may start with cannot reach
    // a match, because the window opens far above the line that matters.
    let buf = String::from_utf8_lossy(&buf);

    // `dsh web: http://127.0.0.1:<port>/?token=<token>`
    let re = regex::Regex::new(r"dsh web:\s+http://127\.0\.0\.1:(\d+)/\?token=(\S+)").ok()?;
    re.captures_iter(buf.as_ref())
        .filter(|cap| cap[1].parse::<u16>() == Ok(port))
        .last()
        .map(|cap| cap[2].to_string())
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{logged_ports, recent_enough, token_from_log, LOG_PORT_MAX_AGE, LOG_TAIL_BYTES};

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
    fn the_token_is_found_even_when_the_window_opens_inside_a_character() {
        let dir = tempfile::tempdir().expect("temp dir");
        let token = "TOKEN-VALUE";

        // A log long enough that the 256 KiB window opens inside it, built from
        // lines that are mostly multi-byte — which is what a real dsh log with
        // Chinese entries looks like.
        let mut log = String::new();
        while log.len() < LOG_TAIL_BYTES as usize + 4096 {
            log.push_str(
                "[dsh-cost-meter] 已加载,账本:C:\\Users\\u\\.dsh\\storages\\cost-meter\\ledger.json\n",
            );
        }
        let line = format!("dsh web: http://127.0.0.1:3080/?token={token}\n");
        // Pad to a length whose window start is a continuation byte: that offset
        // is the one a `from_utf8`-checked read used to reject outright.
        let padding = (0..128usize)
            .find(|pad| {
                let from = log.len() + pad + line.len() - LOG_TAIL_BYTES as usize;
                log.as_bytes()
                    .get(from)
                    .is_some_and(|byte| (0x80..=0xBF).contains(byte))
            })
            .expect("a mostly multi-byte log always has such an offset");
        log.push_str(&"x".repeat(padding));
        log.push_str(&line);
        std::fs::write(dir.path().join("server-3080.out.log"), &log).expect("write");

        assert_eq!(token_from_log(dir.path(), 3080), Some(token.to_string()));
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
}
