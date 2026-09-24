//! Core logic shared by any DSH desktop shell.
//!
//! This crate deliberately has **no GUI dependency**. A Tauri/wry shell needs
//! `libwebkit2gtk-4.1-dev` to compile at all; keeping the interesting logic
//! here means it can be built and unit-tested on a machine that only has a
//! Rust toolchain.
//!
//! Everything about running dsh locally is split by what it is about, and the
//! dependencies run one way — [`server`] is the only module that combines them:
//!
//! * [`console`] — decoding what a child process wrote to its console: UTF-8
//!   when it is, the machine's own code page when a Windows message is not.
//! * [`paths`] — where dsh, `node` and `npm` are. Files only: no network, no
//!   processes.
//! * [`ports`] — what is on a port, whether a server could bind it, and the port
//!   the user last asked for.
//! * [`logs`] — the launcher log directory and the startup token read back out
//!   of it.
//! * [`handshake`] — dsh's two-step browser handshake, walked in Rust and
//!   reduced to a [`handshake::Session`].
//! * [`launch`] — spawning `dsh web` detached, and waiting for its UI.
//! * [`server`] — the layer above those: what is already running, what a port
//!   would do, and starting on the one that was chosen.
//! * [`updates`] — read the published `@deepseek-ai/dsh` versions, split them
//!   into the stable / rc / alpha channels, and remember which versions the
//!   user asked not to be reminded about.
//! * [`self_update`] — the same question about *this* application, answered from
//!   its own GitHub releases rather than from npm.
//! * [`zoom`] — the guest window's zoom factor: its bounds, the value to start
//!   at, and the file it is remembered in.
//! * [`geometry`] — the same question about the guest window's own shape: where
//!   it was, whether that is still a place a window can be put, and the file it
//!   is remembered in.

pub mod console;
pub mod geometry;
pub mod handshake;
pub mod launch;
pub mod logs;
pub mod paths;
pub mod ports;
pub mod self_update;
pub mod server;
pub mod updates;
pub mod zoom;
