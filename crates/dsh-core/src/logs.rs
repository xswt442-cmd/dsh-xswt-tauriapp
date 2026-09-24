//! The launcher log: where a spawned server's stdout goes, and where its startup
//! token is read back from.
//!
//! This directory is not dsh's — dsh does not write it and does not know the
//! name. It is an external convention: wherever a launcher sends a spawned
//! server's output is where its token can be recovered from, and the file names
//! are part of that convention rather than this application's to change.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bytes of log tail scanned for the startup token.
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// How many lines of a failed start's stderr a failure message carries.
const ERR_TAIL_LINES: usize = 5;
/// How many characters of it: the message is rendered in a dialog rather than
/// read in a terminal, and a stack trace can be enormous.
const ERR_TAIL_CHARS: usize = 600;

/// The stdout log for `port` — the file the startup token is read back from.
pub fn out_path(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("server-{port}.out.log"))
}

/// The stderr log for `port`.
///
/// A separate file on purpose: the stdout log carries the launch token, so it is
/// never the one quoted into a message a user reads or forwards.
pub fn err_path(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("server-{port}.err.log"))
}

/// The startup token for `port`, read from the tail of its server log.
///
/// Only the tail matters: the newest entry for the port is the live token.
pub fn token_from_log(dir: &Path, port: u16) -> Option<String> {
    let mut handle = fs::File::open(out_path(dir, port)).ok()?;
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
    token_in(&String::from_utf8_lossy(&buf), port)
}

/// The startup token for `port` inside `text`, newest match wins.
///
/// Split out of [`token_from_log`] so the same reading serves a window that
/// begins at a byte offset, which is how [`token_since`] answers "did *this*
/// run write one".
pub fn token_in(text: &str, port: u16) -> Option<String> {
    // `dsh web: http://127.0.0.1:<port>/?token=<token>`
    let re = regex::Regex::new(r"dsh web:\s+http://127\.0\.0\.1:(\d+)/\?token=(\S+)").ok()?;
    re.captures_iter(text)
        .filter(|cap| cap[1].parse::<u16>() == Ok(port))
        .last()
        .map(|cap| cap[2].to_string())
}

/// The startup token written for `port` since byte `from`.
///
/// Nothing rotates these logs and they are only ever appended to, so a token in
/// the tail may belong to an *earlier* run on the same port. Telling "this launch
/// never wrote one" apart from "this launch's token was refused" needs a window
/// that starts where the launch did.
pub fn token_since(dir: &Path, port: u16, from: u64) -> Option<String> {
    read_since(&out_path(dir, port), from).and_then(|text| token_in(&text, port))
}

/// The last few non-empty lines `port`'s server wrote to stderr since `from`.
///
/// Decoded with [`crate::console::decode`] rather than lossily: a Windows dsh
/// writes its own errors in the machine's code page, and those are exactly the
/// lines a failure message ends up quoting.
pub fn stderr_since(dir: &Path, port: u16, from: u64) -> Option<String> {
    let bytes = read_since_bytes(&err_path(dir, port), from)?;
    tail_lines(&crate::console::decode(&bytes))
}

/// A file from byte `from` to its end, or `None` when there is nothing new there.
fn read_since(path: &Path, from: u64) -> Option<String> {
    read_since_bytes(path, from).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The bytes of the above, for a caller that decodes them itself.
fn read_since_bytes(path: &Path, from: u64) -> Option<Vec<u8>> {
    let mut handle = fs::File::open(path).ok()?;
    let len = handle.metadata().ok()?.len();
    if len <= from {
        return None;
    }
    handle.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::new();
    handle.read_to_end(&mut buf).ok()?;
    (!buf.is_empty()).then_some(buf)
}

/// The last few non-empty lines of `text`, or `None` when it has none.
///
/// Pure, so the caps are pinned without a file: this ends up in a dialog, and a
/// failing server must not be able to make that dialog unreadable.
fn tail_lines(text: &str) -> Option<String> {
    let mut lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    if lines.len() > ERR_TAIL_LINES {
        lines.drain(..lines.len() - ERR_TAIL_LINES);
    }
    let mut tail = lines.join("\n");
    let excess = tail.chars().count().saturating_sub(ERR_TAIL_CHARS);
    if excess > 0 {
        // Cut from the front: the end of a stack trace is where the reason is.
        tail = format!("…{}", tail.chars().skip(excess).collect::<String>());
    }
    Some(tail)
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

    use super::{
        logged_ports, out_path, recent_enough, stderr_since, tail_lines, token_from_log, token_in,
        token_since, ERR_TAIL_CHARS, ERR_TAIL_LINES, LOG_PORT_MAX_AGE, LOG_TAIL_BYTES,
    };

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

    #[test]
    fn a_token_from_a_previous_run_is_not_this_run_s_token() {
        // The logs are appended to and never rotated, so "the port's token" and
        // "the token this launch wrote" are different questions — and the failure
        // message has to answer the second one to mean anything.
        let dir = tempfile::tempdir().expect("temp dir");
        let log = out_path(dir.path(), 3080);
        std::fs::write(&log, "dsh web: http://127.0.0.1:3080/?token=OLD\n").expect("write");
        let from = std::fs::metadata(&log).expect("metadata").len();

        // Nothing new yet: this run has written no token.
        assert_eq!(token_since(dir.path(), 3080, from), None);
        // The tail still finds the old one, which is why the offset matters.
        assert_eq!(token_from_log(dir.path(), 3080).as_deref(), Some("OLD"));

        std::fs::write(
            &log,
            "dsh web: http://127.0.0.1:3080/?token=OLD\ndsh web: http://127.0.0.1:3080/?token=NEW\n",
        )
        .expect("write");
        assert_eq!(token_since(dir.path(), 3080, from).as_deref(), Some("NEW"));
        // Another port's line in the same window is not this port's token.
        assert_eq!(
            token_in("dsh web: http://127.0.0.1:3081/?token=X", 3080),
            None
        );
    }

    #[test]
    fn the_stderr_tail_is_this_run_s_lines_and_stays_readable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = dir.path().join("server-3080.err.log");
        std::fs::write(&log, "an earlier run's dying words\n").expect("write");
        let from = std::fs::metadata(&log).expect("metadata").len();
        assert_eq!(stderr_since(dir.path(), 3080, from), None);

        let lines: Vec<String> = (1..=ERR_TAIL_LINES + 5)
            .map(|n| format!("line {n}"))
            .collect();
        std::fs::write(
            &log,
            format!("an earlier run's dying words\n{}\n", lines.join("\n")),
        )
        .expect("write");

        let tail = stderr_since(dir.path(), 3080, from).expect("this run wrote lines");
        assert!(!tail.contains("earlier run"), "{tail}");
        assert_eq!(tail.lines().count(), ERR_TAIL_LINES);
        assert!(
            tail.ends_with(&format!("line {}", ERR_TAIL_LINES + 5)),
            "{tail}"
        );

        // A single enormous line is cut from the front, where a stack trace is
        // least informative, and the mark says so.
        let huge = format!("{}\n", "x".repeat(ERR_TAIL_CHARS * 3));
        std::fs::write(&log, huge).expect("write");
        let tail = stderr_since(dir.path(), 3080, 0).expect("a line");
        assert_eq!(tail.chars().count(), ERR_TAIL_CHARS + 1);
        assert!(tail.starts_with('…'), "{tail}");
    }

    #[test]
    fn a_tail_with_nothing_in_it_is_no_tail() {
        assert_eq!(tail_lines(""), None);
        assert_eq!(tail_lines("\n\n   \n"), None);
        // Trailing whitespace goes, leading whitespace stays: a stack trace is
        // read by its indentation.
        assert_eq!(tail_lines("one\n\n two \n").as_deref(), Some("one\n two"));
    }

    #[test]
    fn a_missing_or_empty_log_answers_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(stderr_since(dir.path(), 3080, 0), None);
        assert_eq!(token_since(dir.path(), 3080, 0), None);
        std::fs::write(dir.path().join("server-3080.err.log"), "").expect("write");
        assert_eq!(stderr_since(dir.path(), 3080, 0), None);
    }
}
