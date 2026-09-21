//! Bring a dsh web UI up exactly the way the shell does, and print its session.
//!
//! The compat workflow runs this to prove that discovery, the launch-token
//! handshake and the server spawn still work against a given dsh build, without
//! needing the GUI toolchain:
//!
//! ```text
//! cargo run --example launch
//! ```
//!
//! Progress goes to stderr. stdout carries the whole session as `key=value`
//! lines, so a caller can capture it with `$(...)` and still be able to read the
//! cookie:
//!
//! ```text
//! url=http://127.0.0.1:3080/
//! cookie=dsh-auth-web=v1.…
//! ```
//!
//! `url` is deliberately the *clean* address: the token handshake is walked in
//! Rust and consumed there, so a non-empty `cookie` is the evidence that it
//! happened. Exits non-zero, with the reason on stderr, when the UI never comes
//! up.

use dsh_xswt_tauriapp_core::{launch, server};

fn main() {
    let launch = match server::launch_with_progress(launch::BOOT_TIMEOUT_SECS, |message| {
        eprintln!("[launch] {message}");
    }) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let session = launch.session();
    eprintln!(
        "[launch] {} the server on :{}",
        if launch.is_reused() {
            "reused"
        } else {
            "started"
        },
        session.port
    );
    eprintln!(
        "[launch] session {}",
        if session.is_authenticated() {
            "authenticated"
        } else {
            "unauthenticated (the server does not require a cookie)"
        }
    );

    println!("url={}", session.url);
    println!("cookie={}", session.cookie.as_deref().unwrap_or(""));
    println!("port={}", session.port);
}
