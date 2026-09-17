//! What the shell knows, and what it is willing to tell a page.
//!
//! Two audiences, deliberately kept apart:
//!
//! * [`ShellState`] is the snapshot the **bootstrap page** renders. It is
//!   serialised to a webview, so nothing secret may go in it — in particular not
//!   the dsh session cookie.
//! * [`Shell::session`] holds the prepared dsh session for Rust's own use. The
//!   cookie is a credential for the dsh origin and has no business in a page,
//!   not even the shell's own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dsh_xswt_tauriapp_core::{self_update, server, updates};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Progress text while the shell comes up.
pub const EVENT_STATUS: &str = "shell://status";
/// The dsh session is reachable; carries the full [`ShellState`].
pub const EVENT_READY: &str = "shell://ready";
/// Bringing the UI up failed; carries the full [`ShellState`].
pub const EVENT_ERROR: &str = "shell://error";
/// Result of an update check; carries an [`crate::update::UpdatePayload`].
pub const EVENT_UPDATE: &str = "shell://update";
/// Startup reached the point where the user picks a port and a version; carries
/// the full [`ShellState`].
pub const EVENT_CHOOSE: &str = "shell://choose";
/// Result of a check for a newer build of *this application*; carries a
/// [`crate::update::SelfUpdatePayload`].
pub const EVENT_SELF_UPDATE: &str = "shell://self-update";

/// The window showing the shell's own UI: progress, updates, failures. A local
/// origin, and the only window a capability is granted to.
pub const BOOTSTRAP_LABEL: &str = "bootstrap";
/// The window showing dsh. A remote origin with no IPC back to the shell.
pub const GUEST_LABEL: &str = "dsh";

/// Where the shell is in its own startup.
///
/// Serialised lowercase, because the bootstrap page matches on the string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Finding a server, or starting one the user already asked for.
    #[default]
    Starting,
    /// Discovery is done and the user has not chosen a port yet. Nothing is
    /// started in this phase: which port to use is the user's call, so the
    /// server is only spawned once they make it.
    Choosing,
    /// A prepared session exists.
    Ready,
    /// Startup failed; [`ShellState::error`] says why.
    Failed,
}

/// Everything the bootstrap page needs to render, in one serialisable snapshot.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ShellState {
    /// Where startup has got to.
    pub phase: Phase,
    /// Human-readable progress line.
    pub message: String,
    /// The dsh UI address, once known. The *clean* one: the launch token is
    /// consumed in Rust and never reaches a page.
    pub url: Option<String>,
    /// Port the UI is served on.
    pub port: Option<u16>,
    /// Whether an existing server was reused rather than started.
    pub reused: bool,
    /// The port the field offers when the user does not care. Rendered as the
    /// input's placeholder, so it is grey and costs nothing to accept.
    pub default_port: Option<u16>,
    /// An already-running server the shell can enter, if there is one. Distinct
    /// from `port`, which is only set once a session exists.
    pub running_port: Option<u16>,
    /// The resolved dsh launcher, so the dialog's "installed at" line shows a
    /// path rather than, as it used to, the loopback URL.
    pub dsh_bin: Option<String>,
    /// This build's own version, so the page can name what it would update.
    pub shell_version: Option<String>,
    /// The last self-update check, as the page sees it.
    pub self_update: Option<crate::update::SelfUpdatePayload>,
    /// Fatal startup error, when `phase` is `failed`.
    pub error: Option<String>,
    /// The installed dsh version.
    pub current_version: Option<String>,
    /// Directory holding `server-<port>.{out,err}.log`, shown on failure.
    pub log_dir: Option<String>,
    /// The latest update check result.
    pub update: Option<updates::UpdateReport>,
    /// Why the last update check failed, if it did.
    pub update_error: Option<String>,
}

/// How far handing the guest window its session has got.
///
/// The guest is created hidden and shown only from [`Handoff::Priming`] on the
/// first finished page load, so a failed hand-off shows up in the bootstrap
/// window rather than as a broken dsh window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Handoff {
    /// No guest window yet.
    #[default]
    Idle,
    /// The guest exists and its first load is in flight.
    Priming,
    /// The guest has been shown.
    Done,
    /// It did not work; the reason is in [`ShellState::error`].
    Failed,
}

/// Process-wide shell state.
pub struct Shell {
    /// The snapshot the bootstrap page renders.
    pub state: ShellState,
    /// The do-not-remind list.
    pub store: updates::DismissStore,
    /// Where the do-not-remind list is persisted.
    pub dismiss_path: Option<PathBuf>,
    /// The verified dsh session. Rust-only: see the module docs.
    pub session: Option<server::Session>,
    /// How far the guest window hand-off has got.
    pub handoff: Handoff,
    /// The port the user last chose by hand.
    pub port_memory: server::PortMemory,
    /// Versions of *this application* the user asked not to be reminded about.
    /// A second store rather than a shared one: dsh versions and application
    /// versions are different namespaces and must not silence each other.
    pub self_dismiss: updates::DismissStore,
    /// The pending self-update, kept in Rust because acting on it needs the
    /// asset URLs — which have no business in a page.
    pub self_pending: Option<self_update::Available>,
    /// Zoom factor of the guest window, tracked here because the webview has no
    /// getter for it.
    pub zoom: f64,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            state: ShellState::default(),
            store: updates::DismissStore::default(),
            dismiss_path: None,
            session: None,
            handoff: Handoff::default(),
            port_memory: server::PortMemory::default(),
            self_dismiss: updates::DismissStore::default(),
            self_pending: None,
            zoom: 1.0,
        }
    }
}

/// The shared handle every thread works through.
pub type SharedShell = Arc<Mutex<Shell>>;

/// The current snapshot, for the page to pull after it has attached listeners.
pub fn snapshot(shell: &SharedShell) -> ShellState {
    shell
        .lock()
        .map(|guard| guard.state.clone())
        .unwrap_or_default()
}

/// Record and broadcast a progress line.
pub fn set_message(app: &AppHandle, shell: &SharedShell, message: &str) {
    let snap = match shell.lock() {
        Ok(mut guard) => {
            guard.state.message = message.to_string();
            guard.state.clone()
        }
        Err(_) => return,
    };
    let _ = app.emit(EVENT_STATUS, snap);
}

/// Give up: record the reason, tell the bootstrap page, and surface it.
///
/// The bootstrap window is what the user reads, so it is shown as well — a
/// failure that happened after the hand-off began would otherwise be invisible,
/// with the guest window hidden or already gone.
pub fn fail(app: &AppHandle, shell: &SharedShell, error: impl Into<String>) {
    let message = error.into();
    let snap = match shell.lock() {
        Ok(mut guard) => {
            guard.handoff = Handoff::Failed;
            guard.state.phase = Phase::Failed;
            guard.state.message = "启动失败".into();
            guard.state.error = Some(message.clone());
            guard.state.clone()
        }
        Err(_) => return,
    };
    crate::shell_log!("[dsh-harness] failed: {message}");
    let _ = app.emit(EVENT_ERROR, snap);
    crate::guest::retire(app);
    if let Some(window) = app.get_webview_window(BOOTSTRAP_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
