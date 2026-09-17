# Changelog

Release notes are generated from the matching version section; newest first.
For Chinese, see [CHANGELOG.md](CHANGELOG.md).

## Unreleased

### Added

- The port can be chosen at launch. The dialog carries a port field under the version columns, with the default shown in grey (the first free port of `3080`–`3129`, or one typed before); confirming accepts it, and any other port can be typed instead.
- Port availability is decided up front and told apart in four ways: reusable / owned by another program / a dsh this machine cannot enter (a Windows-side instance, or one under another `DSH_HOME`) / below `1024` — instead of being discovered by a failed start.
- A port typed by hand is remembered and offered as the next default.

### Changed

- The dialog now appears on every launch (titled "启动 dsh", primary button "打开 dsh"). It used to appear only when an update existed, so there was no moment at which a port could be chosen. The update check runs alongside the port scan, so confirming never waits on the registry.
- Discovery candidates now come from the file names in `$DSH_HOME/launcher/logs` rather than a sweep of `3080`–`3129`, and the scan uses a shorter connect timeout of its own. That is what lets an unusual port be found again, and it removes the ~40 second cold start measured on Windows with no server running.

### Fixed

- A menu or tray that fails to build no longer stops the application from starting. Both are conveniences, and `setup` used to treat their failure as a failed launch — fatal on a desktop without a StatusNotifier host, and on the macOS menu path that had never been run.
- The dialog's "installed at" line showed the loopback URL; it now shows the resolved dsh launcher.

## 0.0.7 - 2026-09-17

### Fixed

- The app and its installers now carry the project's own icon. `src-tauri/icons/` was still the Tauri scaffold's blue rounded square — byte for byte the same in the repository, the exe and the installed copy — and Windows had a second problem: `bundle.windows.nsis` named no icon, so the installer script's `INSTALLERICON` was an empty string and both `setup.exe` and `uninstall.exe` fell back to NSIS's own icon. The set is now generated from the branding image with `tauri icon` (`icon.ico` carries 16/24/32/48/64/256) and both NSIS icon settings point at it.
- The release workflow moved to each action's Node 24 release (`upload-artifact@v6`, `download-artifact@v7`, `pnpm/action-setup@v5`), which clears the Node 20 deprecation warnings.

## 0.0.6 - 2026-09-16

### Fixed

- Windows no longer stops on the `bootstrap` page. `open_dsh`, which creates the `dsh` window, was a synchronous command, so it ran inside the webview's own IPC callback — and on Windows building a second webview from there deadlocks (wry#583): the window is created and never handed back, so the `dsh` window stayed hidden. It is an `async` command now, and the window is still created on the main thread. The 0.0.5 Windows installer installed and started, but its interface never reached dsh.
- Shortcuts can no longer be held for good. A refused registration is normal — a bare `F12` is commonly taken by another application — and the release path used `unregister_all`, which stops at the first key it cannot unregister and leaves every key after it held. Each binding is now released and reported on its own, and the held count names the keys that actually registered.
- The reason a hand-off failed now reaches the shell's log. A rejected `open_dsh` only rendered the failure page, and a packaged GUI has no terminal, so the one failure that decides whether the app is usable left no trace; it now goes to stderr through `page_diag`, like the other failures.
- The local `npm run build` no longer hardcodes Linux's bundler targets (`deb,rpm,appimage`) and follows `tauri.conf.json`'s `targets` instead, so it is usable on Windows and macOS too.

## 0.0.5 - 2026-09-16

### Changed

- The shell is now a desktop harness for dsh: Tauri owns windows, processes, launching and reuse, the session hand-off, menus/tray/shortcuts, updates, external links and failure recovery, while the page belongs entirely to dsh.
- Startup is split across two windows: `bootstrap` carries progress, the update dialog and failures, while the `dsh` window shows the dsh interface alone. Nothing from startup can be drawn over dsh — that is a structural guarantee, not a convention.
- The session hand-off moved into Rust: the new `resolve_session()` walks and verifies the handshake and returns a clean address plus the session cookie, so the window receives an already-prepared session and the launch token never enters a page URL or `location.search`.
- The first navigation is now host-initiated. dsh's `SameSite=Strict` is unchanged; the cookie being withheld on cross-site navigation is solved by the order of the hand-off instead of by relaxing dsh's security semantics.
- Added menus, a tray and shortcuts: macOS uses a native system menu (including the Edit menu the standard text shortcuts depend on), Windows and Linux use a tray menu, and shortcuts are registered only while one of the shell's windows has focus.
- Zoom now goes through the native `set_zoom` from Rust, and DevTools come from the `devtools` feature, hidden from the menu in release builds by default.

### Removed

- Removed the `initialization_script` injected into dsh's page, the 401 text sniffing, the failed-hand-off sentinel path, and the page's `Ctrl+R` listener. The shell no longer depends on any of the guest's front-end structure.

### Fixed

- A hand-off no longer strands the window on dsh's 401 page: the session cookie is written into the cookie store and read back before the dsh window is created, supplying the `Domain` the server omitted.
- The `dsh` window stays hidden until its page has loaded, and a load that times out returns to the `bootstrap` page with the reason rather than leaving an empty window.
- compat CI now asserts that the example prints a clean address with no token in it, and that the same address still answers 401 without the cookie.
- Windows builds no longer fail over `global-hotkey`'s manager, which is a bare `HWND` there — neither `Send` nor `Sync`, so it can never be Tauri managed state (a `Mutex` would not help either: `Mutex<T>: Sync` needs `T: Send`). The manager now lives on the thread that created it, and managed state carries only plain data.
- Shortcuts on Linux can no longer register successfully and then never fire. `global-hotkey` grabs through X11, and a Wayland-native window's keys never reach the X server — that is Wayland's design. The shell now takes the X11 backend (XWayland) whenever `DISPLAY` exists, with `DSH_SHELL_WAYLAND=1` to opt out.
- compat CI compiles the harness on Windows and macOS. It had only ever been compiled on Linux, so a platform-specific type difference could not surface until a release was being bundled. A fired shortcut also logs itself now, which separates "registered but never fires" from "nobody pressed anything".

## 0.0.4 - 2026-09-16

### Added

- `Ctrl+R` / `F5` to reload, because the window has no browser chrome. Some dsh settings — the content font size, and the theme's boot values — are written into the index when the host renders it, and nothing changes them after the page loads, so those settings need a reload to take effect. The shell offered no way to reload at all.

### Fixed

- The injected script no longer tries to call the shell's IPC from a remote page (the dsh UI). It is refused there, and a rejected promise would re-enter the same handler and loop. Diagnostics are now reported only from the shell's own page; a remote page keeps just the one signal it needs, the failed hand-off.

## 0.0.3 - 2026-09-16

### Fixed

- A failed hand-off no longer leaves a blank window. dsh's session cookie carries `SameSite=Strict`, and the shell page to dsh is a cross-site navigation, so the request after the first 303 went out without that cookie and the window stopped on dsh's 401 text. The injected script now recognises that page and navigates to a sentinel path; the shell intercepts it, resolves the address again and retries — by then the browser is already on the dsh origin, the navigation is same-site, and the cookie is sent.

## 0.0.2 - 2026-09-16

### Fixed

- Launched from a desktop menu or file manager, `node` and `npm` are now derived from dsh's own install location rather than the first ones on `PATH`. The first one there is usually an older system node (v18 on this machine), which cannot start a dsh server at all, and its `npm install -g` targets `/usr/local` — not writable by an ordinary user — so "update and restart" neither updated nor explained itself.
- Landing on dsh's authentication page no longer leaves a blank window: the address is resolved again and retried once, and a second failure returns to the shell page with the reason and a concrete next step.

## 0.0.1 - 2026-09-15

### Added

- Server reuse and startup: scans ports 3080–3129, reuses a running `dsh web`, and otherwise starts one on the first free port in its own process group.
- Two-step launch-token handshake: reads the token from the server log, redeems it with `GET /?token=…`, re-requests with the session cookie, and checks the page marker; when the handshake cannot be completed, another service on the port is not mistaken for dsh.
- Update check on launch: reads the published versions of `@deepseek-ai/dsh` from npm and shows them in stable, RC and alpha columns, with the option to install one and restart.
- Update candidates come only from channels at least as stable as the installed version, so an RC or stable install is never prompted towards alpha.
- "Don't remind me about this version" records the version in the app config directory and suppresses only that version.
- Links leaving the app are opened by the desktop's default handler.
- Windows installer, macOS dmg, deb, rpm and AppImage bundles; the release profile disables debug info and enables LTO and size optimisation.

### Security

- The Tauri capability covers only the shell's own local page. The dsh interface loaded after hand-off is a remote origin and gains no access to the shell's IPC surface.
- Installing an update invokes the `npm` next to the resolved `node`, rather than whichever `npm` happens to be on `PATH` with a different prefix.

## License

[MIT](./LICENSE)
