//! The platform's menu surface.
//!
//! macOS gets a real system menu, because that is the native thing there and the
//! only way `Cmd+C` reaches a webview at all. Windows and Linux get a tray
//! instead: a menu attached to a window *is* a visible menu bar on those
//! platforms, and it would eat a strip of dsh's height for the whole session.

use tauri::{AppHandle, Wry};

use crate::guest;
use crate::menu::actions::{
    accelerator_for, friendly_accelerator, Action, ID_DEVTOOLS, ID_QUIT, ID_RELOAD, ID_SHOW,
    ID_ZOOM_IN, ID_ZOOM_OUT, ID_ZOOM_RESET,
};

/// The first submenu is the application menu; the others are the conventional
/// `编辑` (without which the standard text shortcuts do nothing in a webview) and
/// `窗口` menus.
#[cfg(target_os = "macos")]
pub(super) fn build_app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<Wry>> {
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
pub(super) fn build_tray_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<Wry>> {
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
