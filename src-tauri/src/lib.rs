//! Tauri harness for DeepSeek Harness.
//!
//! The harness does not touch dsh's source, and it does not touch dsh's page. It
//! owns everything *around* the runtime — windows, the server process, the launch
//! handshake, the native menu and tray, shortcuts, updates, external links,
//! DevTools, failure recovery — while dsh owns its own content. Nothing here
//! injects a script into the dsh origin, reads its DOM, patches its CSS, or draws
//! shell UI over it.
//!
//! Two windows, and the split is what makes that guarantee structural rather than
//! a promise:
//!
//! * `bootstrap` — the shell's own page: progress, the update dialog, failures.
//!   A local origin, and the only window a capability is granted to. It is the
//!   only thing the user sees until dsh is ready.
//! * `dsh` — the guest. Built hidden, pointed straight at a prepared session, and
//!   shown once it has loaded. No shell UI is ever placed on it.
//!
//! Because the bootstrap page is a separate window it can never end up drawn over
//! dsh; because the guest is created *after* the session cookie is in the jar, its
//! first navigation is a host-initiated one, which is what lets dsh's
//! `SameSite=Strict` cookie be sent. See [`guest`] for why that matters.

mod bootstrap;
mod commands;
mod guest;
mod menu;
mod state;
mod update;

use std::sync::{Arc, Mutex};

use dsh_xswt_tauriapp_core::{server, updates};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Log a harness event.
///
/// On in debug builds, and in release builds when `DSH_SHELL_DEBUG` is set — a
/// packaged GUI app has no terminal, so "why did it not hand over" would
/// otherwise be unanswerable in the field.
macro_rules! shell_log {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) || std::env::var_os("DSH_SHELL_DEBUG").is_some() {
            eprintln!($($arg)*);
        }
    };
}
pub(crate) use shell_log;

/// Build the shell's own window.
///
/// Built here rather than declared in `tauri.conf.json`, because a
/// config-created window cannot carry a navigation policy. Note what is *not*
/// here: an `initialization_script`. Nothing is injected into any page.
fn build_bootstrap(app: &tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        state::BOOTSTRAP_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("DeepSeek Harness（外壳）")
    // Sized for the dialog, which is what this window now shows: it used to be
    // sized for a splash that the dialog then covered.
    .inner_size(960.0, 730.0)
    .min_inner_size(640.0, 520.0)
    .center()
    .resizable(true)
    // The frame before the page paints. Slightly lifted towards the page's own
    // glow, so the hand-over from the OS fill to the rendered gradient is not a
    // visible step.
    .background_color(tauri::window::Color(0x17, 0x1b, 0x28, 0xff))
    .on_navigation(|url| {
        // The page is local; anything else is a link out.
        if guest::is_internal(url) {
            return true;
        }
        guest::open_external(url.as_str());
        false
    })
    .on_new_window(|url, _features| {
        guest::open_external(url.as_str());
        tauri::webview::NewWindowResponse::Deny
    })
    .build()?;
    Ok(())
}

/// Assemble the process-wide shell state, loading the do-not-remind list.
fn new_shell(app: &tauri::AppHandle) -> state::SharedShell {
    let dismiss_path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("dismissed-updates.json"));
    let store = dismiss_path
        .as_ref()
        .map(updates::DismissStore::load)
        .unwrap_or_default();
    let port_path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("last-port.json"));
    let port_memory = port_path
        .as_ref()
        .map(server::PortMemory::load)
        .unwrap_or_default();
    shell_log!(
        "[dsh-harness] dismiss store {} -> {:?}",
        dismiss_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".into()),
        store.dismissed,
    );

    Arc::new(Mutex::new(state::Shell {
        state: state::ShellState {
            phase: state::Phase::Starting,
            message: "正在启动…".into(),
            log_dir: Some(server::log_dir().display().to_string()),
            // Resolved once here rather than per check: the dialog's "installed
            // at" line used to show the loopback URL, which is not where dsh is.
            dsh_bin: server::resolve_dsh_bin().map(|path| path.display().to_string()),
            ..Default::default()
        },
        store,
        dismiss_path,
        port_memory,
        ..Default::default()
    }))
}

/// Prefer the X11 backend on Linux when an X display exists.
///
/// Not cosmetic. `global-hotkey` grabs keys through X11, and a Wayland-native
/// window's keyboard input never passes through the X server — the shortcuts
/// would register successfully and then never fire. XWayland is present on every
/// Wayland desktop, so taking the X11 path costs some rendering polish and buys
/// back reload, zoom and the inspector.
///
/// An explicit `GDK_BACKEND` always wins, and `DSH_SHELL_WAYLAND=1` opts out.
/// With no `DISPLAY` at all there is nothing to fall back to, so nothing changes.
#[cfg(target_os = "linux")]
fn prefer_x11() {
    if std::env::var_os("GDK_BACKEND").is_some() || std::env::var_os("DSH_SHELL_WAYLAND").is_some()
    {
        return;
    }
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    std::env::set_var("GDK_BACKEND", "x11");
    shell_log!("[dsh-harness] using the X11 backend, so the keyboard shortcuts can be grabbed");
}

/// Every other platform renders one way, so there is nothing to choose.
#[cfg(not(target_os = "linux"))]
fn prefer_x11() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything initialises GTK.
    prefer_x11();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let shell = new_shell(&handle);
            app.manage(shell.clone());
            // Acquiring global shortcuts can fail on a desktop that will not
            // hand them out. That is a lost gesture, not a failed launch, so the
            // manager degrades to `None` and the harness carries on.
            app.manage(menu::Shortcuts::install());

            // The bootstrap window is the one thing there is no substitute for;
            // without it there is nothing to show, so this one may fail startup.
            build_bootstrap(app)?;

            // The menu and the tray are conveniences, and a desktop that will
            // not give us one — no StatusNotifier host, no indicator library —
            // must not stop dsh from coming up. Same rule the shortcuts already
            // follow, and the reason the macOS menu is not a startup risk.
            if let Err(error) = menu::install(&handle, &shell) {
                shell_log!("[dsh-harness] no menu or tray on this desktop: {error}");
            }
            menu::watch_hotkeys(&handle);

            // Discovery and the registry fetch block. They run on their own
            // thread so the window keeps painting.
            let worker_app = handle.clone();
            std::thread::spawn(move || bootstrap::discover(worker_app, shell));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::check_port,
            commands::start_server,
            commands::open_dsh,
            commands::page_diag,
            commands::check_updates,
            commands::dismiss_version,
            commands::clear_dismissed,
            commands::dismissed_versions,
            commands::apply_update,
            commands::restart_app,
            commands::dismiss_path,
        ])
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(focused) => {
                // Shortcuts are held only while one of our windows has focus.
                // See `menu` for why they are not held permanently.
                if let Some(shortcuts) = window.app_handle().try_state::<menu::Shortcuts>() {
                    shortcuts.hold(*focused);
                }
            }
            WindowEvent::CloseRequested { .. } => {
                // Closing a window quits the harness. The dsh server is detached
                // and keeps running — stopping it is the instance manager's job,
                // not this shell's.
                window.app_handle().exit(0);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running the DSH Tauri harness");
}
