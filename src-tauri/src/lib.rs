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
    .inner_size(860.0, 660.0)
    .min_inner_size(560.0, 420.0)
    .center()
    .resizable(true)
    .background_color(tauri::window::Color(0x14, 0x14, 0x14, 0xff))
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
            ..Default::default()
        },
        store,
        dismiss_path,
        ..Default::default()
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let shell = new_shell(&handle);
            app.manage(shell.clone());
            // Acquiring global shortcuts can fail on a desktop that will not
            // hand them out. That is a lost gesture, not a failed launch, so the
            // manager degrades to `None` and the harness carries on.
            app.manage(menu::Shortcuts::install());

            build_bootstrap(app)?;
            menu::install(&handle, &shell)?;
            menu::watch_hotkeys(&handle);

            // Discovery, the server spawn and the registry fetch all block. They
            // run on their own thread so the window keeps painting.
            let worker_app = handle.clone();
            std::thread::spawn(move || bootstrap::run(worker_app, shell));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
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
