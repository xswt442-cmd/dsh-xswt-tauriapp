# Changelog

Release notes are generated from the matching version section; newest first.
For Chinese, see [CHANGELOG.md](CHANGELOG.md).

## Unreleased

### Added

- The dsh window's size and position are remembered and restored, and a remembered position is used only while a display that is here now can still show it.
- A second launch raises the shell that is already running instead of opening a second window; `DSH_SHELL_ALLOW_MULTIPLE=1` runs several at once.

### Fixed

- A failed start is no longer one sentence for three different things: a process that exited, a service listening without writing a token, and a handshake that did not complete are told apart — the last names the installed dsh version — and the tail of this run's stderr comes with it.
- The self-update installer is kept in the cache directory, so the `sudo apt install <path>` in the message still resolves after a reboot; the directory keeps only the newest installer.
- Windows hands the installer over with `ShellExecuteW` and reads the answer: `explorer` only forwards the request and its exit code says nothing about the file, so a machine that could not open it reported "installer opened" too.
- The marketplace stub keeps its installer under `$DSH_HOME`, beside the state it records, and removes the installers a newer download supersedes: it used a temporary directory, so the path in that state was gone after a reboot while the stub stayed silent about it.

## 0.0.12 - 2026-09-24

### Fixed

- dsh 0.1.7-rc.1 redirects the token exchange to a relative `./`, which the shell joined straight onto the origin (`http://127.0.0.1:3600./`) and failed the handshake on — a server that was listening reported as "not ready"; the redirect is now resolved against the request URL.

## 0.0.11 - 2026-09-22

### Fixed

- Updating dsh on Windows no longer fails on a `\\?\` path: the npm derived from the launcher loses that prefix before it reaches the command line, which is the only form `cmd.exe` can run.
- A failed update no longer explains itself in replacement characters: a child's output is decoded as UTF-8 first, and as the machine's OEM code page when that fails — which is where Windows writes its own messages.
- On Linux and WSL the shell's own update no longer claims a success it did not have: the hand-over waits for `xdg-open` and, when nothing opens the `.deb` (exit 3), says so and names `sudo apt install <path>`.

## 0.0.10 - 2026-09-21

### Added

- The dsh window's zoom factor is remembered across restarts; `DSH_SHELL_ZOOM` seeds it for a session and wins over the remembered value.

### Fixed

- Update checks and "update and restart" run off the webview's IPC callback, so the window stays responsive.
- The marketplace stub takes the host platform and architecture as parameters; `node --test` passes on all three platforms.
- Picking a port binds it instead of probing, so a listener that is still settling cannot be misread as free.
- Candidate ports come only from logs written in the last 90 days.
- A port typed by hand is always remembered; install targets are checked as semver; the zoom factor carries into the new dsh window.
- The stub's "already installed" answer comes from the fixture, so tests no longer flip on a machine that has the shell.
- The stub recognises WSL, where `xdg-open` cannot install a `.deb`, and points at the Windows installer instead.
- An opener that exits non-zero is logged, so a failed hand-off is no longer silent.
- Updating dsh on Windows no longer fails with `os error 193`: the npm launcher is chosen per platform and a `.cmd` runs through `cmd.exe /d /s /c`.
- The npm prefix accepts both `<prefix>/lib/node_modules` and `<prefix>/node_modules`, so Windows derives the npm that owns dsh too.
- The log tail is read as bytes and decoded leniently, so a multi-byte character cannot swallow the launch token.
- The hand-off is primed before the window is built, so no hidden window and no false load timeout.

### Security

- The `bootstrap` window admits only bundled assets; no other loopback page reaches the command surface.
- External links on Windows open through `explorer`: a `&` in a URL is no longer a command boundary and `%VAR%` is not expanded.
- The dsh window admits only the session it was handed; other loopback links open in the browser.

### Maintenance

- Linux names the `sudo apt install` command after handing over the installer.
- `docs:check` verifies the five version fields agree; the README environment table lists `DSH_SHELL_ZOOM`.

## 0.0.9 - 2026-09-19

### Added

- The marketplace entry declares storefront screenshots: `plugins/dsh-desktop-app/screenshots.json` and `assets/screenshot-1-launcher.png`.

### Maintenance

- Release notes are generated from the default branch's `CHANGELOG.md` rather than the tag's, so a corrected older entry is no longer undone by a re-run.
- Changelog entries are one line each and state the change; the README puts platform support before the install instructions.

## 0.0.8 - 2026-09-17

### Added

- The port can be chosen at launch: the default is the running dsh's port if there is one, otherwise the first free port of `3080`–`3129` or the one typed last; any other port can be typed instead.
- Port availability is decided before launch and reported as will-start, reusable, occupied, not enterable (a dsh whose session this machine cannot adopt), or below `1024`.
- A port typed by hand is remembered, ahead of the scan while it is still free.
- Self-update: a new version is offered above the version columns, and this platform's installer is picked from the repository's releases and handed over only once it matches the published `SHA256SUMS`; with none for this platform the release page opens instead, and a bad or missing checksum file is refused without writing anything.
- A marketplace entry `plugins/dsh-desktop-app/`: a zero-dependency stub declaring `dsh.bundle` that fetches `SHA256SUMS` on the first dsh start and writes this platform's installer only once the digest matches, then hands it to the system installer; an existing install stays silent, no desktop session or no installer here only gets one line, and nothing is installed silently or imports a harness API.

### Changed

- The dialog now appears on every launch ("启动 dsh" / "打开 dsh"), and the update check runs alongside the port scan.
- Discovery candidates now come from the file names in `$DSH_HOME/launcher/logs` instead of a sweep of `3080`–`3129`, with its own shorter connect timeout for the scan; an unusual port is found again next launch.

### Fixed

- A menu or tray that fails to build no longer stops the application from starting, which it did on a desktop without a StatusNotifier host.
- The dialog's "installed at" line shows the resolved dsh launcher rather than the loopback URL.
- Version comparison strips the tag's `v` prefix and requires both sides to parse as semver, so the running build is no longer offered back as an update.

### Maintenance

- The release workflow gained a `stub` job that attaches the marketplace stub as the version-free `dsh-xswt-tauriapp-plugin.tgz` and asserts the tarball carries `dsh.bundle` and its `cordis.patch.yml`; the tag contract now covers five version fields, adding `plugins/dsh-desktop-app/package.json`.
- The duplicate `## Unreleased` sections — two in each changelog — are merged into one; `scripts/release-notes.mjs` takes only the first match.

## 0.0.7 - 2026-09-17

### Fixed

- The app and its installers carry the project's own icon now: the set is generated with `tauri icon` (`icon.ico` up to 256px), and the NSIS installer and uninstaller icons are named.
- The release workflow moved to each action's Node 24 release, clearing the Node 20 deprecation warnings.

## 0.0.6 - 2026-09-16

### Fixed

- Windows no longer stops on the `bootstrap` page without reaching dsh: the command that creates the window is now `async`, with the window still built on the main thread.
- Shortcuts are unregistered one by one, so a refused registration no longer leaves the rest held for good.
- A failed hand-off reaches the shell's log through `page_diag` — stderr in debug builds, or in release builds with `DSH_SHELL_DEBUG` set — instead of only rendering the failure page.
- The local `npm run build` follows `tauri.conf.json`'s `targets` instead of hardcoding Linux's bundler targets.

## 0.0.5 - 2026-09-16

### Changed

- The shell is now a desktop harness for dsh: Tauri owns windows, the server process, launch and reuse, the session hand-off, menus/tray/shortcuts, updates, external links and failure recovery; the page belongs to dsh.
- Startup is split across two windows: `bootstrap` carries progress, the update dialog and failures, while the `dsh` window shows dsh alone; nothing from startup can be drawn over dsh, which is a structural guarantee rather than a convention.
- The hand-off moved into Rust: `resolve_session()` verifies the handshake and returns a clean address and the session cookie, so the launch token never enters a page URL.
- The first navigation is host-initiated, leaving dsh's `SameSite=Strict` unchanged.
- Menus, a tray and shortcuts: a native system menu on macOS (including the Edit menu), a tray menu on Windows and Linux, with shortcuts registered only while one of the shell's windows has focus.
- Zoom goes through the native `set_zoom` from Rust, and DevTools come from the `devtools` feature, hidden from the menu in release builds.

### Removed

- The `initialization_script` injected into dsh's page, the 401 text sniffing, the failed-hand-off sentinel path and the page's `Ctrl+R` listener are gone; the shell no longer depends on the guest's front-end structure.

### Fixed

- A hand-off no longer strands on dsh's 401 page: the session cookie is written into the store and read back before the window is created, supplying the `Domain` the server omits.
- The `dsh` window stays hidden until its page loads, and a timeout returns to `bootstrap` with the reason.
- compat CI asserts the example prints a clean, token-free address and that it still answers 401 without the cookie.
- Windows builds no longer fail over `global-hotkey`'s manager: it now lives on the thread that created it (`thread_local`), and managed state carries plain data only.
- Linux shortcuts no longer register and then never fire: the X11 backend (XWayland) is used whenever `DISPLAY` exists, with `DSH_SHELL_WAYLAND=1` to opt out.
- compat CI now compiles the harness on Windows and macOS, and a fired shortcut logs itself.

## 0.0.4 - 2026-09-16

### Added

- `Ctrl/Cmd+R` to reload, since the window has no browser chrome and some dsh settings (content font size, the theme's boot values) only take effect on reload.

### Fixed

- The injected script no longer calls the shell's IPC from the remote dsh page; diagnostics are reported from the shell's own page only.

## 0.0.3 - 2026-09-16

### Fixed

- The retry after a failed hand-off now happens on the dsh origin, so the `SameSite=Strict` session cookie travels with it instead of stopping on the 401 text.

## 0.0.2 - 2026-09-16

### Fixed

- Launched from a desktop menu or file manager, `node` and `npm` are derived from dsh's own install location rather than the first ones on `PATH`.
- Landing on dsh's authentication page no longer leaves a blank window: the address is retried once, and a second failure returns to the shell page with the reason and a next step.

## 0.0.1 - 2026-09-15

### Added

- Server reuse and startup: scans `3080`–`3129`, reuses a running `dsh web`, otherwise starts one on the first free port in its own process group.
- Two-step launch-token handshake: reads the token from the server log, redeems it with `GET /?token=…`, re-requests with the session cookie and checks the page marker; when it cannot be completed, another service on the port is not mistaken for dsh.
- Update check on launch: reads the published `@deepseek-ai/dsh` versions from npm and shows them in stable, RC and alpha columns, with install-and-restart.
- Update candidates come only from channels at least as stable as the installed version.
- "Don't remind me about this version" is recorded per version in the app config directory and suppresses only that one.
- Links leaving the app are opened by the desktop's default handler.
- Windows installer, macOS dmg, deb, rpm and AppImage bundles; the release profile disables debug info and enables LTO and size optimisation.

### Security

- The Tauri capability covers only the shell's own local page; the dsh interface loaded after hand-off is a remote origin and gains no IPC access.
- Installing an update invokes the `npm` next to the resolved `node`, rather than whichever `npm` is on `PATH`.

## License

[MIT](./LICENSE)
