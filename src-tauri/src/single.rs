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
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                // Stop rather than spin. This loop has no deadline, so a failure
                // that keeps failing would otherwise cost one line of stderr per
                // iteration for the life of the process. Dropping the listener
                // releases the name, which is exactly the state a shell with no
                // single-instance support is in: the convenience is lost, the
                // launch never is.
                shell_log!(
                    "[dsh-harness] single-instance accept failed ({error}); no longer listening"
                );
                return;
            }
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
        // are different applications as far as the socket is concerned. Pinned to
        // the spelling each build must produce — comparing against a second copy
        // of the same `cfg!` branch would hold however `socket_name` itself
        // changed, which is to say it would pin nothing.
        let base = "com.xswt.dsh.tauri";
        if cfg!(debug_assertions) {
            assert_eq!(socket_name(base), format!("{base}-dev"));
        } else {
            assert_eq!(socket_name(base), base);
        }
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
        //
        // The accept has to be underway before the connect, which is why this
        // waits on a thread like the shell does: on Windows a synchronous
        // listener hands out its pipe instance from inside `accept`, so a client
        // that connects first is waiting for the very instance its own `accept`
        // would create. Sequenced on one thread the two deadlock, and the test
        // hung there for the life of the run.
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
        let (said, heard) = std::sync::mpsc::channel();
        let accepted = std::thread::spawn(move || {
            let Ok(mut stream) = listener.accept() else {
                return;
            };
            let mut hello = [0u8; HELLO.len()];
            if std::io::Read::read_exact(&mut stream, &mut hello).is_ok() {
                let _ = said.send(hello);
            }
        });

        assert!(hand_over(&identifier), "the listener is there to be found");
        let hello = heard
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the hello arrives within 10 seconds");
        assert_eq!(&hello[..], HELLO);

        // Once the listener is gone the same name reads as a first launch again,
        // which is what makes the next launch of the shell a fresh one rather than
        // a hand over to a process that has quit. The listener went down with the
        // thread, so joining is how this knows that.
        let _ = accepted.join();
        assert!(!hand_over(&identifier));
    }
}
