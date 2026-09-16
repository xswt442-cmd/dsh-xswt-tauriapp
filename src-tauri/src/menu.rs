//! Native menus, the tray, and keyboard shortcuts.
//!
//! dsh's window has no browser chrome, so the gestures a browser would provide —
//! reload, zoom, the inspector — have to come from somewhere. Where they come
//! from depends on the platform, because Tauri's menu support is not symmetric:
//!
//! * **macOS** gets a real system menu. That is the native thing there, and it is
//!   also the only way `Cmd+C` / `Cmd+V` reach a webview at all.
//! * **Windows and Linux** get a tray menu instead. A menu attached to a window
//!   *is* a visible menu bar on those platforms, and it would eat a strip of
//!   dsh's height for the whole session. There is no way to have accelerators
//!   without a menu — Tauri's only accelerator machinery is menus — so the
//!   shortcuts are registered as global shortcuts that are held **only while one
//!   of our windows has focus**. Holding them permanently would take `Ctrl+R`
//!   away from every other application on the machine.
//!
//! The shortcuts go through `global-hotkey` directly rather than
//! `tauri-plugin-global-shortcut`, for one reason: the plugin builds its hotkey
//! manager in its own setup and fails the whole application if the desktop
//! refuses — which a Wayland session or a headless environment will. Losing a
//! shortcut must never keep dsh from starting, so [`Shortcuts::install`] treats
//! "no hotkeys here" as a normal outcome.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use global_hotkey::hotkey::HotKey as Shortcut;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tauri::{AppHandle, Manager, Wry};

use crate::guest;
use crate::shell_log;
use crate::state::SharedShell;

/// Menu item ids, shared by the macOS menu and the tray menu so both dispatch
/// through one table.
const ID_RELOAD: &str = "view.reload";
const ID_ZOOM_IN: &str = "view.zoom-in";
const ID_ZOOM_OUT: &str = "view.zoom-out";
const ID_ZOOM_RESET: &str = "view.zoom-reset";
const ID_DEVTOOLS: &str = "view.devtools";
const ID_SHOW: &str = "app.show";
const ID_QUIT: &str = "app.quit";

/// What a menu item or shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
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
const ACCELERATORS: &[(&str, Action)] = &[
    ("CmdOrCtrl+KeyR", Action::Reload),
    ("CmdOrCtrl+Equal", Action::ZoomIn),
    ("CmdOrCtrl+Minus", Action::ZoomOut),
    ("CmdOrCtrl+Digit0", Action::ZoomReset),
    ("F12", Action::DevTools),
];

/// The shortcut spec bound to `action`, for a menu item or a label hint.
fn accelerator_for(action: Action) -> Option<&'static str> {
    ACCELERATORS
        .iter()
        .find(|(_, candidate)| *candidate == action)
        .map(|(spec, _)| *spec)
}

/// Spell a spec the way a user expects to read it.
///
/// A tray menu binds no accelerators, so the shortcut is shown as part of the
/// label instead — which means the hint has to be readable.
fn friendly_accelerator(spec: &str) -> String {
    spec.replace("CmdOrCtrl", "Ctrl")
        .replace("Key", "")
        .replace("Digit", "")
        .replace("Equal", "=")
        .replace("Minus", "-")
}

/// The action a menu id names.
fn action_of_id(id: &str) -> Option<Action> {
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
fn run(app: &AppHandle, shell: &SharedShell, action: Action) {
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

/// The first submenu is the application menu; the others are the conventional
/// `编辑` (without which the standard text shortcuts do nothing in a webview) and
/// `窗口` menus.
#[cfg(target_os = "macos")]
fn build_app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<Wry>> {
    use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

    fn item(
        app: &AppHandle,
        id: &str,
        label: &str,
        action: Action,
    ) -> tauri::Result<MenuItem<Wry>> {
        let mut builder = MenuItemBuilder::new(label).id(id);
        if let Some(spec) = accelerator_for(action) {
            builder = builder.accelerator(spec);
        }
        builder.build(app)
    }

    let separator = || PredefinedMenuItem::separator(app);
    let app_menu = SubmenuBuilder::new(app, "dsh-xswt-tauriapp")
        .item(&PredefinedMenuItem::about(app, None, None)?)
        .item(&separator()?)
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .item(&separator()?)
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    let mut view = SubmenuBuilder::new(app, "视图")
        .item(&item(app, ID_RELOAD, "重新载入", Action::Reload)?)
        .item(&separator()?)
        .item(&item(app, ID_ZOOM_IN, "放大", Action::ZoomIn)?)
        .item(&item(app, ID_ZOOM_OUT, "缩小", Action::ZoomOut)?)
        .item(&item(app, ID_ZOOM_RESET, "实际大小", Action::ZoomReset)?);
    if guest::devtools_available() {
        view =
            view.item(&separator()?)
                .item(&item(app, ID_DEVTOOLS, "开发者工具", Action::DevTools)?);
    }
    let view = view.build()?;

    let edit = SubmenuBuilder::new(app, "编辑")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .item(&separator()?)
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .build()?;

    let window = SubmenuBuilder::new(app, "窗口")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .item(&separator()?)
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &view, &edit, &window])
        .build()
}

// ── the Windows / Linux tray ───────────────────────────────────────────────

/// Flat, with the shortcut spelled out in the label because a tray menu binds no
/// accelerators.
#[cfg(not(target_os = "macos"))]
fn build_tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};

    let label = |text: &str, action: Action| match accelerator_for(action) {
        Some(spec) => format!("{text}    {}", friendly_accelerator(spec)),
        None => text.to_string(),
    };

    let mut builder = MenuBuilder::new(app)
        .item(&MenuItemBuilder::new("显示 dsh").id(ID_SHOW).build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(
            &MenuItemBuilder::new(label("重新载入", Action::Reload))
                .id(ID_RELOAD)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::new(label("放大", Action::ZoomIn))
                .id(ID_ZOOM_IN)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::new(label("缩小", Action::ZoomOut))
                .id(ID_ZOOM_OUT)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::new(label("实际大小", Action::ZoomReset))
                .id(ID_ZOOM_RESET)
                .build(app)?,
        );
    if guest::devtools_available() {
        builder = builder.item(
            &MenuItemBuilder::new(label("开发者工具", Action::DevTools))
                .id(ID_DEVTOOLS)
                .build(app)?,
        );
    }
    builder
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::new("退出").id(ID_QUIT).build(app)?)
        .build()
}

/// Build the platform's menu surface and wire its events.
pub fn install(app: &AppHandle, shell: &SharedShell) -> tauri::Result<()> {
    let events_shell = shell.clone();
    let handle_menu_event = move |app: &AppHandle, event: tauri::menu::MenuEvent| {
        if let Some(action) = action_of_id(event.id().as_ref()) {
            run(app, &events_shell, action);
        }
    };

    #[cfg(target_os = "macos")]
    {
        let menu = build_app_menu(app)?;
        menu.set_as_app_menu()?;
        app.on_menu_event(handle_menu_event);
    }

    #[cfg(not(target_os = "macos"))]
    {
        use tauri::tray::TrayIconBuilder;
        let menu = build_tray_menu(app)?;
        let mut tray = TrayIconBuilder::with_id("dsh-harness")
            .menu(&menu)
            .tooltip("DeepSeek Harness")
            // Left click opens the menu rather than doing something invisible;
            // the window is one item down in it.
            .show_menu_on_left_click(true)
            .on_menu_event(handle_menu_event);
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        tray.build(app)?;
    }

    Ok(())
}

// ── focus-gated shortcuts (Windows / Linux) ────────────────────────────────

/// The global-shortcut manager, when this desktop is willing to give us one.
///
/// `manager: None` is a normal outcome, not an error: a Wayland session or a
/// headless environment refuses global hotkeys, and the harness carries on
/// without them. The tray menu still offers every action.
pub struct Shortcuts {
    manager: Option<GlobalHotKeyManager>,
    bindings: Vec<(Shortcut, Action)>,
    /// Whether the bindings are currently registered, so a repeated focus event
    /// does not try to register them twice.
    held: AtomicBool,
}

impl Shortcuts {
    /// Acquire a hotkey manager, degrading to "no shortcuts" if refused.
    pub fn install() -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(error) => {
                shell_log!("[dsh-harness] this desktop offers no global shortcuts: {error}");
                None
            }
        };
        Self {
            manager,
            bindings: bindings(),
            held: AtomicBool::new(false),
        }
    }

    /// Register the bindings when one of our windows takes focus, and release
    /// them when it loses focus.
    pub fn hold(&self, focused: bool) {
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        // Focus events can repeat; only a change is worth acting on.
        if self.held.swap(focused, Ordering::SeqCst) == focused {
            return;
        }
        if focused {
            for (shortcut, _) in &self.bindings {
                // A desktop environment may already own one of these keys. That
                // costs a gesture, not the session, so it is logged and skipped.
                if let Err(error) = manager.register(*shortcut) {
                    shell_log!("[dsh-harness] could not bind {shortcut:?}: {error}");
                }
            }
        } else {
            let hotkeys: Vec<Shortcut> = self.bindings.iter().map(|(key, _)| *key).collect();
            if let Err(error) = manager.unregister_all(&hotkeys) {
                shell_log!("[dsh-harness] could not release the shortcuts: {error}");
            }
        }
    }

    /// The action a fired shortcut id names.
    fn action_of(&self, id: u32) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(shortcut, _)| shortcut.id == id)
            .map(|(_, action)| *action)
    }
}

/// The bindings, each with the id the runtime will report it back under.
///
/// `HotKey::from_str` always yields id 0, and events arrive carrying only an id,
/// so the numbering happens here.
fn bindings() -> Vec<(Shortcut, Action)> {
    ACCELERATORS
        .iter()
        .enumerate()
        .filter_map(|(index, (spec, action))| {
            let mut shortcut = Shortcut::from_str(spec).ok()?;
            shortcut.id = index as u32 + 1;
            Some((shortcut, *action))
        })
        .collect()
}

/// Point the process-wide hotkey channel at the harness.
///
/// Called once, after [`Shortcuts`] is managed. The channel is a process
/// singleton, which is exactly what is wanted: a fired key arrives here with its
/// id and nothing else.
pub fn watch_hotkeys(app: &AppHandle) {
    let handler_app = app.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        // Fired on press and release; acting on both would reload twice.
        if event.state != HotKeyState::Pressed {
            return;
        }
        let (Some(shortcuts), Some(shell)) = (
            handler_app.try_state::<Shortcuts>(),
            handler_app.try_state::<SharedShell>(),
        ) else {
            return;
        };
        if let Some(action) = shortcuts.action_of(event.id) {
            run(&handler_app, shell.inner(), action);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::{
        accelerator_for, action_of_id, bindings, friendly_accelerator, Action, ACCELERATORS,
    };
    use std::str::FromStr;

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
    fn every_binding_gets_its_own_id_and_maps_back() {
        let bound = bindings();
        assert_eq!(bound.len(), ACCELERATORS.len());
        let shortcuts = super::Shortcuts {
            manager: None,
            bindings: bound,
            held: std::sync::atomic::AtomicBool::new(false),
        };
        // An event carries only an id, so a collision would dispatch the wrong
        // action — or, for id 0, none at all.
        let mut seen = Vec::new();
        for (shortcut, action) in &shortcuts.bindings {
            assert_ne!(shortcut.id, 0, "id 0 is what an unassigned key reports");
            assert!(!seen.contains(&shortcut.id), "duplicate id {}", shortcut.id);
            seen.push(shortcut.id);
            assert_eq!(shortcuts.action_of(shortcut.id), Some(*action));
        }
        assert_eq!(shortcuts.action_of(9999), None);
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
