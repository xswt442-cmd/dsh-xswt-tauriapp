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
mod link;
mod menu;
mod state;
mod update;

use std::sync::{Arc, Mutex};

use dsh_xswt_tauriapp_core::{geometry, paths, ports, self_update, updates, zoom};
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
        // This window carries the only capability, so its policy is its own and
        // narrower than the guest's: the bundled assets, nothing else. A loopback
        // port is not the shell's to load in here.
        if link::is_shell_asset(url) {
            return true;
        }
        link::open_external(url.as_str());
        false
    })
    .on_new_window(|url, _features| {
        link::open_external(url.as_str());
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
        .map(ports::PortMemory::load)
        .unwrap_or_default();

    // The guest window's zoom factor. Remembered because the display scale it
    // has to compensate for is a property of the machine, not of the session —
    // WSLg renders at scale 1 whatever the Windows display scale is — and
    // `DSH_SHELL_ZOOM` pins it for a session configured as a whole.
    let zoom_path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("zoom.json"));
    let zoom_memory = zoom_path
        .as_ref()
        .map(zoom::ZoomMemory::load)
        .unwrap_or_default();
    let zoom_env = std::env::var(zoom::ZOOM_ENV).ok();
    let zoom_factor = zoom::initial(Some(zoom_memory.factor()), zoom_env.as_deref());
    shell_log!(
        "[dsh-harness] guest zoom {zoom_factor} (remembered {}, {})",
        zoom_memory.factor(),
        match zoom_env {
            Some(_) => format!("pinned by {}", zoom::ZOOM_ENV),
            None => "not pinned".to_string(),
        },
    );

    // Where a verified installer is written. The *cache* directory rather than
    // the temporary one, because the dialog shows this path to the user as the
    // argument to `sudo apt install`, and a temporary directory is emptied by a
    // reboot — which is how a download that verified correctly came to look like
    // a file that had never been written.
    let download_dir = app
        .path()
        .app_cache_dir()
        .map(|dir| self_update::downloads_dir(&dir))
        .unwrap_or_else(|_| self_update::fallback_downloads_dir());
    shell_log!(
        "[dsh-harness] installer downloads -> {}",
        download_dir.display()
    );

    // Where the dsh window was. Remembered for the same reason the zoom factor
    // is, and — unlike the factor — checked against the displays that exist *now*
    // when it is used, because a position recorded on a monitor that has since
    // been unplugged is off-screen.
    let window_path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window.json"));
    let window_memory = window_path
        .as_ref()
        .map(geometry::WindowMemory::load)
        .unwrap_or_default();
    shell_log!(
        "[dsh-harness] remembered window geometry {:?}",
        window_memory.geometry()
    );

    // A second store, not a second entry in the first one: "don't remind me
    // about dsh 0.1.5-rc.2" must not silence an update to this application.
    let self_dismiss_path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("dismissed-shell-updates.json"));
    let self_dismiss = self_dismiss_path
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
            log_dir: Some(paths::log_dir().display().to_string()),
            // Resolved once here rather than per check: the dialog's "installed
            // at" line used to show the loopback URL, which is not where dsh is.
            dsh_bin: paths::resolve_dsh_bin().map(|path| path.display().to_string()),
            shell_version: Some(update::shell_version()),
            ..Default::default()
        },
        store,
        dismiss_path,
        port_memory,
        self_dismiss,
        zoom: zoom_factor,
        zoom_memory,
        download_dir,
        window_memory,
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

/// Whether this launch may run beside a shell that is already up.
///
/// One instance is the default, because a second window onto the same dsh is
/// almost always a mis-click rather than a wish — the same server, the same
/// session, two windows. The escape hatch is an environment variable rather than
/// a setting, because the case it exists for is deliberate and rare: two shells
/// side by side, on two ports.
fn multiple_instances_allowed() -> bool {
    std::env::var_os("DSH_SHELL_ALLOW_MULTIPLE").is_some()
}

/// Whether an already-running shell can be reached and raised on this desktop.
///
/// The plugin is registered only where it can work. Its Linux half is D-Bus, and
/// its setup *unwraps* the session connection: on a desktop with no session bus
/// it panics, and in a release build `panic = "abort"` means the shell is simply
/// gone. A convenience must never be able to do that, so the bus is checked for
/// first and its absence costs the feature rather than the launch. Everywhere
/// else the mechanism is the platform's own — a named mutex on Windows,
/// LaunchServices on macOS — and it is always there.
#[cfg(target_os = "linux")]
fn single_instance_supported() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return true;
    }
    // The other place a session bus is looked for, and the one a desktop with
    // `dbus-launch` but no exported variable uses.
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .is_some_and(|dir| dir.join("bus").exists())
}

#[cfg(not(target_os = "linux"))]
fn single_instance_supported() -> bool {
    true
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything initialises GTK.
    prefer_x11();

    let mut builder = tauri::Builder::default();
    if multiple_instances_allowed() {
        shell_log!(
            "[dsh-harness] {} is set: a second shell is allowed",
            "DSH_SHELL_ALLOW_MULTIPLE"
        );
    } else if single_instance_supported() {
        // Registered before anything else, which is what the plugin's own docs ask
        // for: the second launch has to be turned back before it builds a window
        // or starts looking for a server. The raising happens inside the *first*
        // process, which is the half a lock file could not do — see `Cargo.toml`.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            shell_log!("[dsh-harness] another instance started; bringing this one forward");
            guest::show(app);
        }));
    } else {
        shell_log!("[dsh-harness] no session bus: a second launch cannot be turned back");
    }

    builder
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
            commands::check_self_update,
            commands::dismiss_self_version,
            commands::apply_self_update,
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
            // The dsh window's shape is kept as it changes, so the next launch can
            // reopen where the user left it. Only the guest: the bootstrap window
            // is a dialog, sized for the dialog.
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if window.label() == state::GUEST_LABEL {
                    guest::remember_geometry(window.app_handle());
                }
            }
            WindowEvent::CloseRequested { .. } => {
                // Closing a window quits the harness. The dsh server is detached
                // and keeps running. The final shape is read here, throttle or
                // not: a move in the last second before quitting is still the one
                // the user chose.
                guest::remember_geometry_now(window.app_handle());
                window.app_handle().exit(0);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running the DSH Tauri harness");
}
