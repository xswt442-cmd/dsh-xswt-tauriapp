//! What a menu item or a shortcut does, and the accelerators both share.
//!
//! The table feeds two different parsers: `muda`, through Tauri's menu
//! accelerators, and `global-hotkey`. The key names are therefore the canonical
//! `keyboard_types::Code` spellings — the short aliases each parser also takes
//! are not shared between them.

use tauri::AppHandle;

use crate::guest;
use crate::state::SharedShell;

/// Menu item ids, shared by the macOS menu and the tray menu so both dispatch
/// through one table.
pub(super) const ID_RELOAD: &str = "view.reload";

pub(super) const ID_ZOOM_IN: &str = "view.zoom-in";

pub(super) const ID_ZOOM_OUT: &str = "view.zoom-out";

pub(super) const ID_ZOOM_RESET: &str = "view.zoom-reset";

pub(super) const ID_DEVTOOLS: &str = "view.devtools";

pub(super) const ID_SHOW: &str = "app.show";

pub(super) const ID_QUIT: &str = "app.quit";

/// What a menu item or shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Reload,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    DevTools,
    Show,
    Quit,
}

/// Every accelerator the harness binds, and what it does.
///
/// The key names are the canonical `keyboard_types::Code` spellings, because the
/// table feeds two different parsers: `muda` (through Tauri's menu accelerators)
/// and `global-hotkey`. Spelled that way both accept every entry; the short
/// aliases each one also takes are not shared.
pub(super) const ACCELERATORS: &[(&str, Action)] = &[
    ("CmdOrCtrl+KeyR", Action::Reload),
    ("CmdOrCtrl+Equal", Action::ZoomIn),
    ("CmdOrCtrl+Minus", Action::ZoomOut),
    ("CmdOrCtrl+Digit0", Action::ZoomReset),
    ("F12", Action::DevTools),
];

/// The shortcut spec bound to `action`, for a menu item or a label hint.
pub(super) fn accelerator_for(action: Action) -> Option<&'static str> {
    ACCELERATORS
        .iter()
        .find(|(_, candidate)| *candidate == action)
        .map(|(spec, _)| *spec)
}

/// Spell a spec the way a user expects to read it.
///
/// A tray menu binds no accelerators, so the shortcut is shown as part of the
/// label instead — which means the hint has to be readable.
pub(super) fn friendly_accelerator(spec: &str) -> String {
    spec.replace("CmdOrCtrl", "Ctrl")
        .replace("Key", "")
        .replace("Digit", "")
        .replace("Equal", "=")
        .replace("Minus", "-")
}

/// The action a menu id names.
pub(super) fn action_of_id(id: &str) -> Option<Action> {
    match id {
        ID_RELOAD => Some(Action::Reload),
        ID_ZOOM_IN => Some(Action::ZoomIn),
        ID_ZOOM_OUT => Some(Action::ZoomOut),
        ID_ZOOM_RESET => Some(Action::ZoomReset),
        ID_DEVTOOLS => Some(Action::DevTools),
        ID_SHOW => Some(Action::Show),
        ID_QUIT => Some(Action::Quit),
        _ => None,
    }
}

/// Carry out an action against whichever window is current.
pub(super) fn run(app: &AppHandle, shell: &SharedShell, action: Action) {
    match action {
        Action::Reload => guest::reload(app),
        Action::ZoomIn => guest::zoom_in(app, shell),
        Action::ZoomOut => guest::zoom_out(app, shell),
        Action::ZoomReset => guest::zoom_reset(app, shell),
        Action::DevTools => guest::toggle_devtools(app),
        Action::Show => guest::show(app),
        Action::Quit => app.exit(0),
    }
}

// ── the macOS system menu ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{accelerator_for, action_of_id, friendly_accelerator, Action, ACCELERATORS};

    #[test]
    fn every_accelerator_parses() {
        // The table feeds two parsers, and a typo in it would only ever surface
        // as a shortcut that silently does nothing. This pins the one reachable
        // without a GUI.
        assert!(!ACCELERATORS.is_empty());
        for (spec, _) in ACCELERATORS {
            assert!(
                global_hotkey::hotkey::HotKey::from_str(spec).is_ok(),
                "{spec} does not parse"
            );
        }
    }

    #[test]
    fn zoom_in_and_out_are_distinct_bindings() {
        let zoom_in =
            global_hotkey::hotkey::HotKey::from_str(accelerator_for(Action::ZoomIn).unwrap())
                .unwrap();
        let zoom_out =
            global_hotkey::hotkey::HotKey::from_str(accelerator_for(Action::ZoomOut).unwrap())
                .unwrap();
        assert_ne!(
            (zoom_in.mods, zoom_in.key),
            (zoom_out.mods, zoom_out.key),
            "zoom in and zoom out must not share a key"
        );
    }

    #[test]
    fn an_unknown_menu_id_is_a_no_op() {
        assert_eq!(action_of_id("nonsense"), None);
        assert_eq!(action_of_id("view.reload"), Some(Action::Reload));
    }

    #[test]
    fn the_tray_hint_is_readable() {
        // The tray binds no accelerators, so the label carries the shortcut and
        // has to read like one.
        assert_eq!(friendly_accelerator("CmdOrCtrl+KeyR"), "Ctrl+R");
        assert_eq!(friendly_accelerator("CmdOrCtrl+Equal"), "Ctrl+=");
        assert_eq!(friendly_accelerator("CmdOrCtrl+Minus"), "Ctrl+-");
        assert_eq!(friendly_accelerator("CmdOrCtrl+Digit0"), "Ctrl+0");
        assert_eq!(friendly_accelerator("F12"), "F12");
    }
}
