//! Focus-gated keyboard shortcuts (Windows and Linux).
//!
//! Tauri can only bind a shortcut through a menu accelerator, and a window menu
//! is a visible bar on these platforms, so the gestures are registered as real
//! global shortcuts instead — held **only while one of our windows has focus**.
//! Holding them permanently would take `Ctrl+R` away from every other
//! application on the machine.
//!
//! `global-hotkey`'s manager is not `Send`/`Sync` on Windows (it is a bare
//! `HWND`), so it can never be Tauri managed state. Only plain data is managed;
//! the manager lives in a `thread_local`, which is also where both platforms
//! require it.

use std::cell::RefCell;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use global_hotkey::hotkey::HotKey as Shortcut;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tauri::{AppHandle, Manager};

use crate::menu::actions::{run, Action, ACCELERATORS};
use crate::shell_log;
use crate::state::SharedShell;

/// The shortcut bindings, as Tauri managed state.
///
/// Deliberately holds **no** manager. `global-hotkey`'s Windows manager is a bare
/// `HWND`, which is neither `Send` nor `Sync`, so it cannot go into managed state
/// at all — and wrapping it in a `Mutex` would not help, since `Mutex<T>: Sync`
/// needs `T: Send`. Only the plain data lives here; the manager lives in
/// [`MANAGER`], on the thread that created it, which is where it has to be used
/// anyway.
pub struct Shortcuts {
    bindings: Vec<(Shortcut, Action)>,
    /// Whether the bindings are currently registered, so a repeated focus event
    /// does not try to register them twice.
    held: AtomicBool,
}

thread_local! {
    /// The hotkey manager, reachable only from the thread that made it.
    ///
    /// A `thread_local` rather than managed state or a `static`: it needs no
    /// `Send`/`Sync` bound, and both platform implementations require the
    /// creating thread anyway — the Windows one needs that thread's message loop
    /// to receive `WM_HOTKEY`, and macOS wants the main thread for Carbon.
    ///
    /// The *event* handler never touches this: on Linux the X11 backend reports
    /// presses from its own thread, which is exactly why the bindings are kept
    /// separate from the manager.
    static MANAGER: RefCell<Option<GlobalHotKeyManager>> = const { RefCell::new(None) };
}

impl Shortcuts {
    /// Acquire a hotkey manager, degrading to "no shortcuts" if refused.
    ///
    /// `None` is a normal outcome, not an error: a Wayland session or a headless
    /// environment refuses global hotkeys, and the harness carries on without
    /// them. The tray menu still offers every action.
    ///
    /// Must be called on the main thread, because that is the thread the manager
    /// will be used from.
    pub fn install() -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(error) => {
                shell_log!("[dsh-harness] this desktop offers no global shortcuts: {error}");
                None
            }
        };
        MANAGER.with(|slot| *slot.borrow_mut() = manager);
        Self {
            bindings: bindings(),
            held: AtomicBool::new(false),
        }
    }

    /// Register the bindings when one of our windows takes focus, and release
    /// them when it loses focus.
    pub fn hold(&self, focused: bool) {
        // Focus events can repeat; only a change is worth acting on.
        if self.held.swap(focused, Ordering::SeqCst) == focused {
            return;
        }
        MANAGER.with(|slot| {
            let borrowed = slot.borrow();
            let Some(manager) = borrowed.as_ref() else {
                shell_log!("[dsh-harness] no hotkey manager on this desktop; shortcuts stay off");
                return;
            };
            if focused {
                let mut acquired = 0;
                for (shortcut, _) in &self.bindings {
                    // A desktop environment may already own one of these keys.
                    // That costs a gesture, not the session, so it is logged and
                    // skipped rather than raised.
                    match manager.register(*shortcut) {
                        Ok(()) => acquired += 1,
                        Err(error) => {
                            shell_log!("[dsh-harness] could not bind {shortcut:?}: {error}");
                        }
                    }
                }
                shell_log!(
                    "[dsh-harness] shortcuts held ({acquired}/{} bindings)",
                    self.bindings.len()
                );
            } else {
                // Released one at a time on purpose: `unregister_all` stops at the
                // first key it cannot release, so a single key the desktop refused
                // to register would leave every key after it held for good.
                let mut released = 0;
                for (shortcut, _) in &self.bindings {
                    match manager.unregister(*shortcut) {
                        Ok(()) => released += 1,
                        Err(error) => {
                            shell_log!("[dsh-harness] could not release {shortcut:?}: {error}");
                        }
                    }
                }
                if released == self.bindings.len() {
                    shell_log!("[dsh-harness] shortcuts released");
                }
            }
        });
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
pub(super) fn bindings() -> Vec<(Shortcut, Action)> {
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
            // The only trace a shortcut leaves. A grab can be registered and
            // still never fire — in a Wayland-native window, for instance, whose
            // keys never pass through the X server — and without this line that
            // failure is indistinguishable from nobody pressing anything.
            shell_log!("[dsh-harness] shortcut fired: {action:?}");
            run(&handler_app, shell.inner(), action);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::bindings;
    use crate::menu::actions::ACCELERATORS;

    #[test]
    fn every_binding_gets_its_own_id_and_maps_back() {
        let bound = bindings();
        assert_eq!(bound.len(), ACCELERATORS.len());
        let shortcuts = super::Shortcuts {
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
}
