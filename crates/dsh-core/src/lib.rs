//! Core logic shared by any DSH desktop shell.
//!
//! This crate deliberately has **no GUI dependency**. A Tauri/wry shell needs
//! `libwebkit2gtk-4.1-dev` to compile at all; keeping the interesting logic
//! here means it can be built and unit-tested on a machine that only has a
//! Rust toolchain.
//!
//! Two halves:
//!
//! * [`server`] — find or start a local `dsh web` server and complete its
//!   token handshake. Ported from the Electron shell so both behave the same.
//! * [`updates`] — read the published `@deepseek-ai/dsh` versions, split them
//!   into the stable / rc / alpha channels, and remember which versions the
//!   user asked not to be reminded about.
//! * [`self_update`] — the same question about *this* application, answered from
//!   its own GitHub releases rather than from npm.

pub mod self_update;
pub mod server;
pub mod updates;
