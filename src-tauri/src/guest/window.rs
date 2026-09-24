//! The dsh window: built hidden, shown once its first load finishes.
//!
//! No injected script, no DOM reading, no CSS patch, no shell UI drawn over it —
//! everything here is window and process plumbing.

use std::time::{Duration, Instant};

use dsh_xswt_tauriapp_core::geometry;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::guest::session::ensure_cookie;
use crate::link::{is_session, open_external};
use crate::shell_log;
use crate::state::{self, Handoff, SharedShell, BOOTSTRAP_LABEL, GUEST_LABEL};

/// How long the guest's first load may take before it is called a failure.
const GUEST_LOAD_TIMEOUT: Duration = Duration::from_secs(90);
/// The shape the window takes when nothing has been remembered, in logical pixels
/// — which is what the builder takes. A remembered geometry is applied afterwards
/// in physical pixels, the unit it was read in.
const GUEST_WIDTH: f64 = 1500.0;
const GUEST_HEIGHT: f64 = 940.0;
/// How often the window's geometry may be written while it is being dragged.
///
/// These events arrive continuously, and the last one is the one that matters —
/// the close path takes a final reading, throttle or not — so anything more
/// frequent than this is a disk write per pixel of mouse movement.
const GEOMETRY_WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// Create the guest window and point it at the prepared session.
///
/// Must run on the main thread: it builds a window. The cookie work has already
/// happened by now (see the module docs).
pub fn spawn(app: &AppHandle, shell: &SharedShell) -> Result<(), String> {
    if app.get_webview_window(GUEST_LABEL).is_some() {
        // An earlier hand-off already built it; nothing to do.
        return Ok(());
    }
    let session = shell
        .lock()
        .map_err(|_| "状态锁不可用".to_string())?
        .session
        .clone()
        .ok_or_else(|| "dsh 服务尚未就绪".to_string())?;
    let url: tauri::Url = session
        .url
        .parse()
        .map_err(|error| format!("URL 无效：{error}"))?;

    // `Priming` is set before the window exists, not after it. `.build()` returns
    // as soon as the webview does, and the main thread can then dispatch the
    // first `Finished` page load while this thread is still on its way to the
    // lock: `on_loaded` would find `Idle`, decline to show the window, and
    // `watch` would report its 90-second deadline for a page that had already
    // loaded. The zoom factor rides along because it lives in the shell rather
    // than in the webview — see `zoom_by` — so a window built after the last
    // `set_zoom` starts at 1.0 unless it is told.
    let zoom = {
        let Ok(mut guard) = shell.lock() else {
            return Err("状态锁不可用".to_string());
        };
        guard.handoff = Handoff::Priming;
        guard.zoom
    };

    // A remembered geometry is used only when a display that is here *now* would
    // actually show it: a position recorded on a monitor that has since been
    // unplugged would put the window where nobody can reach it, which looks
    // exactly like a shell that did not start.
    let remembered = {
        let found = shell
            .lock()
            .ok()
            .and_then(|guard| guard.window_memory.geometry())
            .filter(|geometry| geometry::lands_on_a_display(geometry, &displays(app)));
        if found.is_some() {
            shell_log!("[dsh-harness] the guest window will reopen where it was");
        }
        found
    };

    let load_shell = shell.clone();
    let session_url = url.clone();
    let window = match WebviewWindowBuilder::new(app, GUEST_LABEL, WebviewUrl::External(url))
        .title("DeepSeek Harness")
        .inner_size(GUEST_WIDTH, GUEST_HEIGHT)
        .min_inner_size(900.0, 600.0)
        .center()
        .resizable(true)
        // Hidden until the page is actually up, so a failed hand-off surfaces in
        // the bootstrap window instead of as a broken dsh window.
        .visible(false)
        .background_color(tauri::window::Color(0x14, 0x14, 0x14, 0xff))
        .on_navigation(move |url| {
            // A link that would replace dsh in the same webview opens in the
            // real browser instead — including one that points at another local
            // dsh, whose session this window cannot enter.
            let internal = is_session(url, &session_url);
            shell_log!(
                "[dsh-harness] guest navigate {} -> {}",
                url,
                if internal { "allow" } else { "open externally" }
            );
            if internal {
                return true;
            }
            open_external(url.as_str());
            false
        })
        // `window.open` / `target="_blank"` — the counterpart of Electron's
        // setWindowOpenHandler. New Tauri webviews on Linux must be created with
        // an explicit related-view link, which is not worth the fragility here,
        // so every popup goes to the system browser.
        .on_new_window(|url, _features| {
            open_external(url.as_str());
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                on_loaded(&window, &load_shell);
            } else {
                shell_log!("[dsh-harness] guest loading {}", payload.url());
            }
        })
        .build()
    {
        Ok(window) => window,
        Err(error) => {
            // Nothing was primed if no window exists, and leaving `Priming`
            // behind would make `watch`'s deadline the next thing to report.
            if let Ok(mut guard) = shell.lock() {
                guard.handoff = Handoff::Idle;
            }
            return Err(format!("无法创建 dsh 窗口：{error}"));
        }
    };
    // Applied after the build rather than through the builder, because the two
    // sides have to agree on units and only the setters do: what
    // `outer_position`/`inner_size` report is physical pixels, which is what
    // `set_position`/`set_size` take, while the builder's `position`/`inner_size`
    // are logical. Round-tripping through the builder would drift the window a
    // little on every launch of a display scaled to anything but 100%. The window
    // is still hidden here, so nothing is seen to move.
    if let Some(geometry) = remembered {
        let _ = window.set_size(tauri::PhysicalSize::new(geometry.width, geometry.height));
        let _ = window.set_position(tauri::PhysicalPosition::new(geometry.x, geometry.y));
    }
    let _ = window.set_zoom(zoom);
    shell_log!("[dsh-harness] guest window created for {}", session.url);

    let repair_app = app.clone();
    std::thread::spawn(move || ensure_cookie(&repair_app, &session));

    // A window that never finishes loading would otherwise leave the user on the
    // bootstrap screen with nothing to read.
    let watch_app = app.clone();
    let watch_shell = shell.clone();
    std::thread::spawn(move || watch(&watch_app, &watch_shell));
    Ok(())
}

/// The guest's first finished load is what puts dsh on screen.
fn on_loaded(window: &WebviewWindow, shell: &SharedShell) {
    let priming = {
        let Ok(mut guard) = shell.lock() else { return };
        if guard.handoff != Handoff::Priming {
            return;
        }
        guard.handoff = Handoff::Done;
        guard.state.message = "dsh 已就绪".into();
        true
    };
    if !priming {
        return;
    }
    shell_log!("[dsh-harness] the guest is up; handing the screen over");
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(bootstrap) = window.app_handle().get_webview_window(BOOTSTRAP_LABEL) {
        let _ = bootstrap.hide();
    }
}

/// Fail the launch if the guest never finishes loading.
fn watch(app: &AppHandle, shell: &SharedShell) {
    let deadline = Instant::now() + GUEST_LOAD_TIMEOUT;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        match shell.lock().map(|guard| guard.handoff) {
            // Done or Failed: somebody else has already settled it.
            Ok(Handoff::Priming) => {}
            _ => return,
        }
        if app.get_webview_window(GUEST_LABEL).is_none() {
            return;
        }
        if Instant::now() >= deadline {
            retire(app);
            state::fail(
                app,
                shell,
                format!(
                    "dsh 界面在 {} 秒内没有加载完成。\n\n\
                     可尝试：重启应用；或先在终端运行 dsh web，确认界面本身可用。",
                    GUEST_LOAD_TIMEOUT.as_secs()
                ),
            );
            return;
        }
    }
}

/// Tear the guest window down without asking, so a failure can fall back to the
/// bootstrap page.
///
/// `destroy` rather than `close`: `close` would raise `CloseRequested`, which the
/// harness reads as "the user closed dsh" and turns into a quit.
pub fn retire(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(GUEST_LABEL) {
        let _ = window.destroy();
    }
}

// ── geometry ──────────────────────────────────────────────────────────────

/// Every display the window could be put on, in the unit the geometry is kept in.
fn displays(app: &AppHandle) -> Vec<geometry::Display> {
    app.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| geometry::Display {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
        })
        .collect()
}

/// Remember where the guest window is, throttled while it is being dragged.
pub fn remember_geometry(app: &AppHandle) {
    remember(app, false);
}

/// Remember it now, throttle or not: the last reading before the window goes.
pub fn remember_geometry_now(app: &AppHandle) {
    remember(app, true);
}

/// Read the guest window's shape and keep it.
///
/// Physical pixels, taken from the frame's position and the client's size —
/// exactly what the next launch hands back to `set_position`/`set_size`.
pub fn remember(app: &AppHandle, force: bool) {
    let Some(shell) = app.try_state::<SharedShell>() else {
        return;
    };
    let Some(window) = app.get_webview_window(GUEST_LABEL) else {
        return;
    };
    let (Ok(size), Ok(position)) = (window.inner_size(), window.outer_position()) else {
        return;
    };
    let geometry = geometry::Geometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let Ok(mut guard) = shell.lock() else {
        return;
    };
    if guard.window_memory.geometry() == Some(geometry) {
        return;
    }
    if !force
        && guard
            .window_saved_at
            .is_some_and(|at| at.elapsed() < GEOMETRY_WRITE_INTERVAL)
    {
        return;
    }
    match guard.window_memory.remember(geometry) {
        Ok(()) => guard.window_saved_at = Some(Instant::now()),
        Err(error) => {
            shell_log!("[dsh-harness] could not remember the window's geometry: {error}");
        }
    }
}

// ── window actions, driven by menu / tray / shortcuts ──────────────────────

/// The window a menu item or shortcut should act on: dsh when it exists, else
/// the bootstrap page.
pub fn target_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(GUEST_LABEL)
        .or_else(|| app.get_webview_window(BOOTSTRAP_LABEL))
}

/// Re-show the guest (or the bootstrap page) and focus it.
pub fn show(app: &AppHandle) {
    if let Some(window) = target_window(app) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Reload the current window.
///
/// This is the gesture a browser would provide and a chrome-less window does
/// not have. It matters: dsh writes the content font size and the theme's boot
/// values into the document when the host renders it, and nothing applies them
/// afterwards, so a settings change needs a reload to take effect.
pub fn reload(app: &AppHandle) {
    if let Some(window) = target_window(app) {
        let _ = window.reload();
    }
}

/// Whether the WebView inspector is reachable in this build.
///
/// The `devtools` Cargo feature makes the API exist; this decides whether a user
/// gets a menu item for it. A release build ships without it unless asked.
pub fn devtools_available() -> bool {
    cfg!(debug_assertions) || std::env::var_os("DSH_SHELL_DEVTOOLS").is_some()
}

/// Show or hide the WebView inspector for the current window.
pub fn toggle_devtools(app: &AppHandle) {
    if !devtools_available() {
        shell_log!("[dsh-harness] DevTools are off in this build");
        return;
    }
    if let Some(window) = target_window(app) {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
    }
}

// ── link policy ────────────────────────────────────────────────────────────
