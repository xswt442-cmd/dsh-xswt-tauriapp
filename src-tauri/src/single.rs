//! One shell at a time.
//!
//! A second launch has to find the shell that is already up and *raise* it: a
//! second window onto the same dsh is the same server and the same session twice
//! over. Finding it is the half a lock file would do; the other half is saying
//! something to it, which needs a channel — and that channel is a local socket: a
//! named pipe on Windows, a Unix-domain socket elsewhere.
//!
//! The first process binds the name and listens; a later one connects, and the
//! first brings itself forward. Nothing here can fail a launch: a name that cannot
//! be claimed is a lost convenience, not a failed start, which is why the pair is
//! not the Tauri plugin (see `Cargo.toml`).
//!
//! # The name
//!
//! Close to the application identifier, plus `-dev` for a debug build. Two
//! different applications must not collide, and neither must the shell being
//! developed and the one that is installed: without the suffix, running the shell
//! from a terminal while the installed copy is up would raise the installed window
//! and silently drop the one under test.

use std::io::{ErrorKind, Write};
use std::thread;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, ToNsName};
use tauri::AppHandle;

use crate::shell_log;

/// What a later launch sends. The content means nothing — the connection *is* the
/// message — but a byte stream wants bytes.
const HELLO: &[u8] = b"show";

/// The name this build claims, and the name a later launch looks for.
fn socket_name(identifier: &str) -> String {
    if cfg!(debug_assertions) {
        format!("{identifier}-dev")
    } else {
        identifier.to_string()
    }
}

/// Ask a shell that is already running to come forward.
///
/// `true` means one answered, and this process has nothing to add. A name nobody
/// is listening on is the normal first launch, not a failure.
pub fn hand_over(identifier: &str) -> bool {
    let name = socket_name(identifier);
    let Ok(name) = name.as_str().to_ns_name::<GenericNamespaced>() else {
        // No namespace, no single instance: the launch carries on.
        shell_log!("[dsh-harness] no single-instance name available; carrying on");
        return false;
    };
    match LocalSocketStream::connect(name) {
        Ok(mut stream) => {
            let _ = stream.write_all(HELLO);
            let _ = stream.flush();
            true
        }
        Err(error) if is_nobody_there(&error) => false,
        Err(error) => {
            // Something is there but would not talk to us. Treating that as "no
            // shell is running" would open a second window; treating it as one
            // would drop this launch. The second is the recoverable mistake, so
            // that is the one made here.
            shell_log!(
                "[dsh-harness] could not reach the running shell ({error}); coming up anyway"
            );
            false
        }
    }
}

/// Claim the name and bring this shell forward whenever a later launch asks.
///
/// Listens on a thread of its own: the accept blocks, and the window has to keep
/// painting. The thread lives as long as the process, which is what the socket is
/// for — it goes away with us, so the next launch finds no one listening.
pub fn listen(app: &AppHandle, identifier: &str) {
    let name = socket_name(identifier);
    let Ok(name) = name.as_str().to_ns_name::<GenericNamespaced>() else {
        shell_log!("[dsh-harness] no single-instance name available");
        return;
    };
    clear_stale(&socket_name(identifier));
    let listener = match ListenerOptions::new().name(name).create_sync() {
        Ok(listener) => listener,
        Err(error) => {
            // The feature is lost, the launch is not. Worth a line: it is the
            // difference between "a second launch raises this window" and
            // "a second launch opens a second window".
            shell_log!("[dsh-harness] could not claim the single-instance name: {error}");
            return;
        }
    };

    let app = app.clone();
    thread::spawn(move || loop {
        match listener.accept() {
            Ok(_) => {
                shell_log!("[dsh-harness] another launch asked for this window");
                crate::guest::show(&app);
            }
            Err(error) => shell_log!("[dsh-harness] single-instance accept failed: {error}"),
        }
    });
}

/// Whether a failed connect means "nobody is listening".
///
/// Both spellings occur: a missing name is `NotFound` on Unix and on Windows,
/// while a socket file left behind by a crash is `ConnectionRefused`.
fn is_nobody_there(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::ConnectionRefused
    )
}

/// Remove a socket file a previous run left behind.
///
/// Only reachable on the Unix platforms that name a local socket by a path
/// (`/tmp/<name>` on macOS and the BSDs): Linux uses the abstract namespace and
/// Windows a named pipe, and both vanish with the process. Without this the first
/// crash would leave the shell unable to claim its own name ever again — the
/// connect above has already established that nothing is listening there.
#[cfg(all(unix, not(target_os = "linux")))]
fn clear_stale(name: &str) {
    let path = format!("/tmp/{name}");
    if std::path::Path::new(&path).exists() {
        shell_log!("[dsh-harness] removing the socket a previous run left at {path}");
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn clear_stale(_name: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name nothing else in this process — or on this machine — is using.
    fn unique(label: &str) -> String {
        format!("com.xswt.dsh.tauri-test-{label}-{}", std::process::id())
    }

    #[test]
    fn a_debug_build_does_not_claim_the_installed_shells_name() {
        // The case this exists for: the shell under test and the installed copy
        // are different applications as far as the socket is concerned.
        assert_eq!(socket_name("com.xswt.dsh.tauri"), {
            if cfg!(debug_assertions) {
                "com.xswt.dsh.tauri-dev".to_string()
            } else {
                "com.xswt.dsh.tauri".to_string()
            }
        });
        assert_ne!(socket_name("x"), socket_name("y"));
    }

    #[test]
    fn nobody_listening_is_a_first_launch_and_not_a_failure() {
        assert!(!hand_over(&unique("idle")));
    }

    #[test]
    fn a_second_launch_reaches_the_one_that_is_listening() {
        // The mechanism end to end: claim the name, then hand a launch over to it.
        // What the first instance *does* with the connection is `listen`'s thread,
        // which wants a window and so is out of reach here — the channel is the
        // part that can be pinned without one.
        let identifier = unique("busy");
        let name = socket_name(&identifier);
        let listener = ListenerOptions::new()
            .name(
                name.as_str()
                    .to_ns_name::<GenericNamespaced>()
                    .expect("namespace"),
            )
            .create_sync()
            .expect("bind the name");

        assert!(hand_over(&identifier), "the listener is there to be found");
        let mut stream = listener.accept().expect("a connection arrives");
        let mut said = [0u8; HELLO.len()];
        std::io::Read::read_exact(&mut stream, &mut said).expect("the hello arrives");
        assert_eq!(&said, HELLO);

        // Once it is gone, the same name reads as a first launch again — which is
        // what makes the next launch of the shell a fresh one rather than a hand
        // over to a process that has quit.
        drop(listener);
        assert!(!hand_over(&identifier));
    }
}
