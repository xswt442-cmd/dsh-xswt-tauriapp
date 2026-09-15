//! Bring a dsh web UI up exactly the way the shell does, and print its URL.
//!
//! The compat workflow runs this to prove that discovery, the launch-token
//! handshake and the server spawn still work against a given dsh build, without
//! needing the GUI toolchain:
//!
//! ```text
//! cargo run --example launch
//! ```
//!
//! Progress goes to stderr; stdout carries the resolved URL alone, so a caller
//! can capture it with `$(...)`. Exits non-zero, with the reason on stderr,
//! when the UI never comes up.

use dsh_xswt_tauriapp_core::server;

fn main() {
    let launch = match server::launch_with_progress(server::BOOT_TIMEOUT_SECS, |message| {
        eprintln!("[launch] {message}");
    }) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    match &launch {
        server::Launch::Reused { port, .. } => eprintln!("[launch] reused the server on :{port}"),
        server::Launch::Started { port, .. } => eprintln!("[launch] started a server on :{port}"),
    }
    println!("{}", launch.url());
}
