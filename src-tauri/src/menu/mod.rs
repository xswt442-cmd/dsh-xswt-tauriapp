//! Native menus, the tray, and keyboard shortcuts.
//!
//! dsh's window has no browser chrome, so the gestures a browser would provide —
//! reload, zoom, the inspector — have to come from somewhere, and where they come
//! from depends on the platform. [`platform`] builds the surface, [`actions`]
//! is the table both the menu and the tray dispatch through, and [`shortcuts`]
//! holds the gestures where there is no menu to hang them on.
//!
//! Nothing here may stop a startup: the surfaces are convenience, and a desktop
//! that will not hand one out must still get dsh.

mod actions;
mod platform;
mod shortcuts;

pub use shortcuts::{watch_hotkeys, Shortcuts};

use crate::state::SharedShell;

/// Build the platform's menu surface and wire its events.
pub fn install(app: &tauri::AppHandle, shell: &SharedShell) -> tauri::Result<()> {
    let events_shell = shell.clone();
    let handle_menu_event = move |app: &tauri::AppHandle, event: tauri::menu::MenuEvent| {
        if let Some(action) = actions::action_of_id(event.id().as_ref()) {
            actions::run(app, &events_shell, action);
        }
    };

    #[cfg(target_os = "macos")]
    {
        let menu = platform::build_app_menu(app)?;
        menu.set_as_app_menu()?;
        app.on_menu_event(handle_menu_event);
    }

    #[cfg(not(target_os = "macos"))]
    {
        use tauri::tray::TrayIconBuilder;
        let menu = platform::build_tray_menu(app)?;
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
