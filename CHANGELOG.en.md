# Changelog

Release notes are generated from the matching version section; newest first.
For Chinese, see [CHANGELOG.md](CHANGELOG.md).

## Unreleased

### Added

- The guest window's zoom factor is remembered: `Ctrl/Cmd+=` / `-` / `0` persist to the application config directory and survive a restart, and `DSH_SHELL_ZOOM` seeds it for a session — the environment wins over the remembered value.

### Fixed

- Update checks and "update and restart" are async commands: a registry request or an `npm install -g` no longer runs inside the webview's IPC callback, so the window stays responsive throughout.
- The marketplace stub takes the host platform and architecture as parameters, so the "no desktop session, do not download" branch is no longer only exercised where the tests happen to run; `node --test` passes on Windows and macOS too.
- Picking a port to start on binds it instead of probing for a listener: while an earlier instance's listener is still settling, a connect probe reports the port free and the child then exited with `EADDRINUSE`.

- Candidate ports come only from logs written within 90 days: nothing prunes them, so every port the machine had ever used was probed on every launch.
- A typed port is marked by the page that typed it, rather than inferred from the value (a port typed by hand that happened to equal the suggestion was not remembered); an install target is checked as a version before it becomes an npm argument; the zoom factor carries into the dsh window instead of being lost at the hand-off.
- The marketplace stub decides "already installed" from the fixture too: the Linux and macOS candidates were absolute paths (`/usr/bin`, `/Applications`), so on a machine that has the shell installed — which is what trying it means — the "not installed, so download" tests started failing (six of them here).
- The marketplace stub recognises WSL, where `xdg-open` cannot install a `.deb`: it says so, and points a Windows desktop at the `*-setup.exe` asset instead.
- A failed hand-off is no longer silent: an opener that exits non-zero is logged, which is how `xdg-open` exits when nothing handles a `.deb`.
- Updating dsh on Windows no longer fails with `os error 193` ("not a valid Win32 application"): npm installs an extensionless POSIX script beside `npm.cmd` and `npm.ps1`, and directory order picked the one that cannot start at all; the executable is chosen per platform now (Windows prefers `npm.cmd`), a `.cmd` runs explicitly through `cmd.exe /d /s /c` with the argv quoted token by token, and no console window flashes.
- The npm prefix is read off the launcher in both directory shapes: npm's Windows `<prefix>\node_modules` has no `lib` level, so the derivation failed there and "the npm that owns dsh" only ever held on Unix; a candidate must also really hold npm, so a module root like `$DSH_HOME/profiles/node_modules` is not mistaken for a prefix.

### Security

- The bootstrap window's navigation policy admits only the bundled assets: it is the only window granted a capability, and any page on any loopback port could previously load there and reach the command surface.
- An external link is no longer opened through `cmd /C start` on Windows: cmd re-parses the string it is handed, so a `&` in a URL's query string ends the command and runs what follows (reachable from any link the dsh page renders), and `%VAR%` is expanded even inside quotes. `explorer` takes the URL as its own argument and does neither.

### Maintenance

- Linux names the `sudo apt install` command after handing the installer over: `xdg-open` on a `.deb` usually reaches an archive manager, not an installer.

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
