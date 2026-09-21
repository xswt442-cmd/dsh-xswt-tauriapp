//! The dsh window, and handing it a session.
//!
//! This is where the harness earns its name. The guest window is dsh's own page
//! and nothing else: no injected script, no DOM reading, no CSS patch, no shell
//! UI drawn over it. What is left here is the split by responsibility —
//! [`session`] writes the session cookie before the window exists, [`window`]
//! builds and shows it, [`zoom`] drives the zoom factor the menu and the
//! shortcuts act on. The link policy is not here: it serves the bootstrap window
//! too, so it lives in [`crate::link`].

pub mod session;
pub mod window;
pub mod zoom;

pub use session::prime_cookie;
pub use window::{devtools_available, reload, retire, show, spawn, toggle_devtools};
pub use zoom::{zoom_in, zoom_out, zoom_reset};
