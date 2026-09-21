//! Walk the port-choice path the launch dialog drives, without a GUI.
//!
//! `plan` → `check_port` → `start_on`, then the same three again from scratch, to
//! show that a deliberately unusual port is discovered on the next launch and not
//! merely used once.
//!
//! ```text
//! cargo run --example port-choice
//! ```
//!
//! It starts two real servers and leaves them running, the way the shell does.
//! Exits non-zero listing everything that did not hold.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use dsh_xswt_tauriapp_core::server::{self, PortChoice};

/// A one-shot HTTP server standing in for whatever occupies a port.
///
/// Serves a handful of requests rather than one: a single classification makes
/// more than one request, and a stand-in that hung up after the first would look
/// like a closed port instead of an answer.
fn stand_in(status: &str, body: &'static str) -> Option<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let status = status.to_string();
    // Detached on purpose, and accepting until the process ends: a stand-in for
    // a program that owns the port has to keep owning it for the whole run. A
    // fixed budget runs out — every probe is a connection — and then the port
    // goes quiet, which makes the next check see a different situation entirely.
    std::thread::spawn(move || loop {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut scratch = [0u8; 1024];
        let _ = stream.read(&mut scratch);
        let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    Some(port)
}

/// A free port outside the band, so the "custom port" case is genuinely custom.
fn free_port_above(start: u16) -> Option<u16> {
    (start..start + 50).find(|port| !server::probe_port(*port, Duration::from_millis(200)))
}

fn report(failures: &mut Vec<String>, ok: bool, label: &str, detail: String) {
    println!("{} {label}: {detail}", if ok { "ok  " } else { "FAIL" });
    if !ok {
        failures.push(format!("{label}: {detail}"));
    }
}

fn main() {
    let mut failures: Vec<String> = Vec::new();

    // ── a port to suggest, and it has to be usable ─────────────────────────
    let plan = match server::plan(None) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let suggested = plan.suggested_port;
    println!(
        "plan: running={:?} suggested={suggested}",
        plan.running.as_ref().map(|session| session.port)
    );
    report(
        &mut failures,
        matches!(
            server::check_port(suggested),
            PortChoice::Start | PortChoice::Reuse(_)
        ),
        "the suggested port is usable",
        format!("{suggested} -> {:?}", server::check_port(suggested)),
    );

    // ── starting on it yields a session, and it becomes discoverable ───────
    let band_port = match server::start_on(suggested) {
        Ok(launch) => launch.port(),
        Err(error) => {
            eprintln!("start_on({suggested}) failed: {error}");
            std::process::exit(1);
        }
    };
    println!("start_on({suggested}) -> {band_port}");

    match server::plan(None) {
        Ok(again) => {
            let found = again.running.as_ref().map(|session| session.port);
            report(
                &mut failures,
                found == Some(band_port),
                "a started server is found again",
                format!("running={found:?}"),
            );
        }
        Err(error) => report(&mut failures, false, "re-plan", error),
    }
    report(
        &mut failures,
        matches!(server::check_port(band_port), PortChoice::Reuse(_)),
        "it is recognised as reusable",
        format!("{band_port} -> {:?}", server::check_port(band_port)),
    );

    // ── a port something else owns is reported, never silently moved ───────
    // A plain listener stands in for "some other program": reachable, but not a
    // dsh the shell could enter.
    match stand_in("200 OK", "hello") {
        Some(busy) => {
            report(
                &mut failures,
                server::check_port(busy) == PortChoice::Occupied,
                "a port another program owns is Occupied",
                format!("{busy} -> {:?}", server::check_port(busy)),
            );
            let refused = server::start_on(busy).is_err();
            report(
                &mut failures,
                refused,
                "starting on it is refused, not moved elsewhere",
                format!("start_on({busy}) errored: {refused}"),
            );
        }
        None => report(&mut failures, false, "bind a stand-in", "no port".into()),
    }

    // ── a dsh this machine cannot enter says so, not "occupied" ────────────
    // The shape of a dsh started on the other side of a WSL boundary: listening
    // on the same loopback, its session held somewhere this shell cannot read.
    match stand_in(
        "401 Unauthorized",
        "dsh web authentication required; reopen the URL printed by dsh web.",
    ) {
        Some(foreign) => report(
            &mut failures,
            server::check_port(foreign) == PortChoice::Foreign,
            "a dsh that cannot be entered is Foreign, not Occupied",
            format!("{foreign} -> {:?}", server::check_port(foreign)),
        ),
        None => report(
            &mut failures,
            false,
            "bind a dsh stand-in",
            "no port".into(),
        ),
    }

    // ── below the bindable range, answered without touching the network ────
    report(
        &mut failures,
        server::check_port(80) == PortChoice::TooLow,
        "a privileged port is TooLow",
        format!("80 -> {:?}", server::check_port(80)),
    );

    // ── a custom port works, and is discovered again next time ────────────
    // This is the case that a band-only scan could never find: the port is
    // outside 3080–3129, and only the log file name records that it was used.
    match free_port_above(32100) {
        Some(custom) => {
            match server::start_on(custom) {
                Ok(launch) => println!("start_on({custom}) -> {}", launch.port()),
                Err(error) => {
                    report(&mut failures, false, "start on a custom port", error);
                    report(
                        &mut failures,
                        false,
                        "custom port discovery",
                        "not attempted".into(),
                    );
                    finish(&failures);
                    return;
                }
            }
            match server::plan(None) {
                Ok(again) => {
                    let found = again.running.as_ref().map(|session| session.port);
                    report(
                        &mut failures,
                        found == Some(custom),
                        "an out-of-band port is found again",
                        format!("{custom} found={found:?}"),
                    );
                }
                Err(error) => report(&mut failures, false, "custom port discovery", error),
            }
        }
        None => report(
            &mut failures,
            false,
            "find a custom port",
            "none free above 32100".into(),
        ),
    }

    finish(&failures);
}

fn finish(failures: &[String]) {
    println!();
    if failures.is_empty() {
        println!("the port-choice path holds");
        return;
    }
    eprintln!("{} check(s) failed:", failures.len());
    for failure in failures {
        eprintln!("  - {failure}");
    }
    std::process::exit(1);
}
