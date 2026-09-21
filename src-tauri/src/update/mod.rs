//! Updates, from two sources that share a dialog and nothing else.
//!
//! [`dsh`] is about the **runtime this shell supervises**: what npm has
//! published, and how to install one of those versions. [`shell`] is about
//! **this application**: what its own GitHub releases say, and how to hand a
//! verified installer to the operating system.
//!
//! They are kept apart for the same reason [`crate::state`] keeps two dismiss
//! stores. A version of dsh and a version of this shell are different things
//! from different sources, so one of them must never answer for the other —
//! and nothing is shared between the two: different registry, different
//! comparison rules, different install mechanism, different event.
//!
//! The update question is a *harness* concern — it is about the runtime this
//! shell supervises, not about anything dsh renders — so it is asked and
//! answered in the bootstrap window, before dsh is ever shown.
//!
//! Re-exported flat, because "which of the two" is already in the name of
//! every caller: `refresh`/`install` are dsh's, `refresh_self` and
//! `apply_self_update` are this application's.

mod dsh;
mod shell;

pub use dsh::{install, refresh};
pub use shell::{apply_self_update, refresh_self, shell_version};
