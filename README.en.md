# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
[![DSH](https://img.shields.io/badge/DSH-desktop%20harness-4d6bfe)](https://github.com/deepseek-ai/deepseek-harness)
[![release](https://img.shields.io/github/v/release/xswt442-cmd/dsh-xswt-tauriapp?label=release&color=2ea043)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest)
[![DSH compatibility](https://img.shields.io/badge/DSH-%3E%3D0.1.5--rc.1-4d6bfe)](#platform-and-compatibility)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078d4)](#platform-and-compatibility)
[![runtime](https://img.shields.io/badge/runtime-system%20WebView-8957e5)](#platform-and-compatibility)
[![compat](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml/badge.svg)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml)
[![downloads](https://img.shields.io/github/downloads/xswt442-cmd/dsh-xswt-tauriapp/total?label=downloads)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases)

A lightweight Tauri desktop shell for DeepSeek Harness, and a **desktop harness / runtime supervisor** for dsh: Tauri owns dsh's periphery, including windows, the server process, launching and reuse, the session hand-off, menus and tray, shortcuts, updates, external links and failure recovery, while dsh owns its own page. The shell does not modify dsh: it does not inject a script into dsh's page, read its DOM, patch its styles or draw shell UI over it, and it takes no part in dsh's session, sandbox or permission model.

## Features

- **Reuse or start a server**: reuses a running `dsh web` in `3080`–`3129`, or starts one on the first free port; the server process is independent of the shell, so closing the window never stops it.
- **Launch-token handshake and session hand-off**: the handshake is walked and verified in Rust, and the launch token never reaches a page's URL.
- **Port chosen at launch**: the default comes from the scan or from the port typed last, and any other port can be typed; availability is answered on the spot.
- **Update prompts**: dsh's updates are shown in stable / RC / alpha columns, while this application's own updates pick and checksum the installer for this platform; each keeps its own "don't remind me" record.
- **System integration**: a native system menu on macOS, a tray menu on Windows and Linux; `Ctrl/Cmd+R`, `Ctrl/Cmd+=` `-` `0` and `F12` are registered only while one of the shell's windows has focus.
- **Links that leave the app** are handed to the desktop's default handler instead of taking over the window.

## Platform and compatibility

| Item | Status |
|---|---|
| Linux (deb / rpm / AppImage) | Supported; produced by CI, and the session hand-off and first navigation have been measured |
| Windows (NSIS installer) | Supported; the installer and GUI are exercised in daily use, and the hand-off and first navigation have been measured |
| macOS (dmg) | Build wired up, unsigned; the GUI has not been exercised |
| WSLg | Runs; WebKitGTK's GPU passthrough is unreliable, so `WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1` are needed. WSLg has no status-bar host, so a tray icon has nowhere to appear; shortcuts work, provided the X11 backend is used. WSLg also renders at scale 1 whatever the Windows display scale is, so on a 125% or 150% display its windows are smaller than native ones: `Ctrl/Cmd+=` corrects that and the factor is remembered, or `DSH_SHELL_ZOOM` pins it |
| dsh | The declared floor is `0.1.5-rc.1`. CI's boot-check covers `0.1.5-rc.1`, `latest` and `0.2.0-rc.1`; installing an older host resolves each component through that host's own caret ranges, so the `0.1.5-rc.1` leg pins its dependency tree with `--before 2026-09-11T00:00:00Z` |

That the first navigation carries the `SameSite=Strict` cookie has been measured on Linux / WebKitGTK and on Windows / WebView2; macOS relies on its webview treating an initiator-less navigation as same-site, which has not been measured.

## Getting it

### From the plugin market

The [marketplace](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin) entry points at `dsh-xswt-tauriapp-plugin.tgz`, which every release of this repository carries. It is an installer stub rather than the application itself (source: [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)):

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

On the **first** dsh start after installing it, the stub reads this repository's latest release, picks the installer for this platform, fetches `SHA256SUMS` and only writes the file once its digest matches, then hands it to the system installer. A machine that already has the shell, or one with no desktop session (`CI`, or Linux without `DISPLAY` and `WAYLAND_DISPLAY`), is only told where to look. The stub reads five variables of its own: `DSH_TAURIAPP_MODE` accepts only `auto`, `notice` and `off`, so anything else is as if unset and the plugin's own config decides; `DSH_TAURIAPP_FORCE=1` makes it download again after having offered once already (only `1` counts, and `notice` mode still only points at the release); `DSH_TAURIAPP_NO_OPEN=1` stops it at the moment it would hand the file over and prints the command to run instead, `1` again being the only value that does; `DSH_TAURIAPP_RELEASES_API` overrides the release endpoint when non-empty; `DSH_TAURIAPP_INSTALL_DIRS` replaces, as a path list, the directories the already-installed probe looks in. Its state file and the installers it downloads live under `$DSH_HOME`, whose non-empty value is honoured as written; the shell's Rust code applies the same test. It installs nothing silently and imports no harness API, so it cannot become the reason a harness fails to start.

### From a release

Each release carries a Windows installer, two macOS dmgs (x64 and aarch64), a deb, an rpm and an AppImage, together with `SHA256SUMS`. The deb depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `libayatana-appindicator3-1` (the tray; on Ubuntu 24.04 and later GTK3 ships as `libgtk-3-0t64`, which `Provides: libgtk-3-0`, so it installs either way):

```sh
sudo apt install ./dsh-xswt-tauriapp_*_amd64.deb
sudo dnf install ./dsh-xswt-tauriapp-*.x86_64.rpm
```

This application's own updater picks only the `.deb` on Linux; the `.rpm` and the AppImage take no part in automatic updates.

### From source

Rust, Node and Tauri's Linux system dependencies are required:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev rpm
pnpm install
pnpm tauri build --bundles deb,rpm,appimage
```

## Usage

```sh
./src-tauri/target/release/dsh-xswt-tauriapp   # dsh-xswt-tauriapp.exe on Windows
```

The shell shows the `bootstrap` page first while it discovers the server and checks for updates, then opens the dsh window. Environment variables:

| Variable | Effect |
|---|---|
| `DSH_HOME` | The DSH home directory: a non-empty value is used as written, whether or not that directory exists yet; only an empty one falls back to `~/.dsh` (the home itself comes from `HOME`, or from `USERPROFILE` on Windows) |
| `DSH_BIN` | When non-empty, the first candidate for dsh's `lib/bin.js`; when that file is not there, the `$DSH_HOME/profiles`, `npm_config_prefix` and `PATH` candidates are still tried |
| `DSH_NODE_BIN` | When non-empty, a candidate for the `node` executable, ahead of `PATH`; the node that owns the installed dsh still wins whenever that node is there |
| `DSH_TAURI_REGISTRY` | Overrides the version endpoint when non-empty; the npm registry otherwise |
| `DSH_SHELL_RELEASES_API` | Overrides the GitHub Releases API this application's own updater reads when non-empty; this repository's `releases/latest` otherwise. For tests and mirrors |
| `DSH_SHELL_ALLOW_MULTIPLE` | Setting it at all lets several shells run at once, an empty value and `0` both counting as set; by default a second launch only raises the one already running |
| `DSH_SHELL_DEBUG` | Setting it at all, even to an empty value, writes the hand-off, navigation and update-check log to stderr; a debug build always does |
| `DSH_SHELL_DEVTOOLS` | Setting it at all, even to an empty value, gives a release build its DevTools; a debug build needs nothing. Where it is unset, neither window carries an inspector and the menu offers no such item, while `F12` stays registered and opens nothing |
| `DSH_SHELL_ZOOM` | The dsh window's initial zoom factor, which wins over the remembered one: it has to parse as a number above `0` (an empty value, `0` and a typo are all ignored, leaving the remembered factor), and an out-of-range one is clamped to 0.3–3.0 |
| `DSH_SHELL_WAYLAND` | On Linux, setting it at all stops the switch to the X11 backend; `0` opts out just as much as `1`, which is only the convention, and an explicit `GDK_BACKEND` is left alone the same way |

## How it works

| Stage | Behaviour |
|---|---|
| Port band | `3080`–`3129`; candidates come from the file names in `$DSH_HOME/launcher/logs/server-<port>.out.log`, a port with no log has no launch token, and a log older than 90 days is not a candidate |
| Reuse | The launch token is read from the tail of that log, `GET /?token=…` returns a 303 with a session cookie, and a second request carrying that cookie checks the page marker; a server without dsh authentication is accepted on a bare `GET /` answering 200 |
| Start | `node <dsh>/lib/bin.js web --port <p> --no-open`, in its own process group, appending to that same log |
| Choosing the port | An adoptable server pre-fills its port and confirming reuses it; otherwise the default is the first free port of the band, and only a typed port is remembered; the prompt also lists up to six logged ports with each one's verdict; below `1024` is refused; a dsh this machine cannot enter is reported as a foreign service |
| Windows | `bootstrap` is a local origin and the only window granted an IPC capability; `dsh` is a remote origin and is granted nothing |
| Hand-off | dsh's session cookie is `SameSite=Strict`, so Rust completes the handshake and writes the cookie first, and builds the dsh window afterwards; that window's first navigation is started by the host and is same-origin |
| Outbound links | Only `http:`, `https:` and `mailto:` reach the desktop's default handler; `file:` and text that does not parse as a URL are refused, with the reason in the shell log (`DSH_SHELL_DEBUG`) |
| Updates | dsh is looked up on npm and this application in its own GitHub Releases, in parallel, and neither compares the other's versions; an installer is checked against `SHA256SUMS` before the system sees it, a mismatch names both hashes and writes nothing, a release without checksums is not downloaded, and a release with nothing installable here offers the release page instead |
| Remembered | A typed port, the zoom factor and the window's size and position live in the application config directory; a remembered position is used only while a display that is here now can show it |

Menu and tray follow the platform: macOS gets the native system menu, whose Edit menu the text shortcuts need, and Windows and Linux get a tray menu with no permanent menu bar. A shortcut is registered only while one of the shell's own windows has focus. On Linux `global-hotkey` grabs keys through X11 and a Wayland-native window's keystrokes never reach the X server, so the shell uses the X11 backend whenever `DISPLAY` exists; `DSH_SHELL_WAYLAND` opts out, and an explicit `GDK_BACKEND` is left alone. Zoom goes through the native `set_zoom` rather than an injected hotkey polyfill.

A failed update check (offline, a rate-limited API, no release yet) stays silent, since not knowing about an update is no reason to interrupt anyone. The launch prompt offers only a channel at least as stable as the installed one, with the channel read from the version string rather than from an npm dist-tag, and "don't remind me" is recorded per version and does not touch dsh's own ignore list.

## Development and verification

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test scripts/changelog.test.mjs
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` has no GUI dependency, so it builds and tests on a machine without `libwebkit2gtk`. `examples/launch.rs` brings a real server up through the same code path the shell uses and prints the prepared session (`url=` and `cookie=`); compat CI uses it to verify the handshake and to pin the fact that the token stays out of the URL. What that example prints as `cookie=` is a live session credential, so it is redacted before any of this reaches a report. The marketplace stub in `plugins/dsh-desktop-app/` is a dependency-free Node module, and every case in its suite runs against a stand-in release server on loopback: nothing reaches GitHub and no installer is opened.

## License

[MIT](./LICENSE)
