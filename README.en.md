# dsh-xswt-tauriapp

**Following the release of the official desktop app, this repository is no longer maintained as of 2026-09-30.**

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
[![DSH](https://img.shields.io/badge/DSH-desktop%20harness-4d6bfe)](https://github.com/deepseek-ai/deepseek-harness)
[![release](https://img.shields.io/github/v/release/xswt442-cmd/dsh-xswt-tauriapp?label=release&color=2ea043)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest)
[![DSH compatibility](https://img.shields.io/badge/DSH-%3E%3D0.1.5--rc.1-4d6bfe)](#platform-and-compatibility)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078d4)](#platform-and-compatibility)
[![runtime](https://img.shields.io/badge/runtime-system%20WebView-8957e5)](#platform-and-compatibility)
[![compat](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml/badge.svg)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/actions/workflows/compat.yml)
[![downloads](https://img.shields.io/github/downloads/xswt442-cmd/dsh-xswt-tauriapp/total?label=downloads)](https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases)

A lightweight Tauri desktop shell for DeepSeek Harness (Web), and a **desktop harness / runtime supervisor** for dsh: the shell owns windows, the server process, the session hand-off, menus and shortcuts, updates and external links, while dsh owns its own page. The shell does not modify dsh and injects no script into its page.

## Features

- **Reuse or start a server**: reuses a running `dsh web` in `3080`–`3129`, or starts one on the first free port; the server process is independent of the shell, so closing the window never stops it.
- **Launch-token handshake and session hand-off**: the token is used only in Rust and never reaches a page's URL.
- **Port chosen at launch**: the default comes from the port scan or from the port typed last, and any other port can be typed.
- **Update prompts**: dsh's updates are shown in stable / RC / alpha columns, and this application's own updates pick and checksum the installer for this platform; each keeps its own "don't remind me" record.
- **System integration**: a native system menu on macOS, a tray menu on Windows and Linux; `Ctrl/Cmd+R`, the zoom keys and `F12` are registered only while a shell window has focus.
- **Links that leave the app** go to the desktop's default handler, and the dsh window keeps showing dsh.

## Platform and compatibility

| Item | Status |
|---|---|
| Linux (deb / rpm / AppImage) | Supported |
| WSLg | Supported; unreliable at times |
| Windows (NSIS installer) | Supported |
| macOS (dmg) | Build wired up, unsigned, GUI not exercised |
| dsh | Floor `0.1.5-rc.1` |

WSLg needs `WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

## Getting it

### From the plugin market

The [marketplace](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin) entry's tarball is `dsh-xswt-tauriapp-plugin.tgz`, which every release of this repository carries. It is the stub that downloads and verifies the installer; the application itself is installed from the same release (source: [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)):

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

On the first dsh start after installing it, the stub reads the latest release, picks the installer for this platform, checks it against `SHA256SUMS` before writing it, and hands it to the system installer. A machine that already has the shell, or one with no desktop session, receives only a sentence. The stub reads these variables:

| Variable | Effect |
|---|---|
| `DSH_TAURIAPP_MODE` | `auto`, `notice` or `off`; any other value leaves the plugin's own config in charge |
| `DSH_TAURIAPP_FORCE` | `1` downloads again after the installer has already been offered once |
| `DSH_TAURIAPP_NO_OPEN` | `1` stops before the hand-over and prints the command to run instead |
| `DSH_TAURIAPP_RELEASES_API` | Overrides the release endpoint |
| `DSH_TAURIAPP_INSTALL_DIRS` | Replaces the directories the already-installed probe looks in |

Its state file and the installers it downloads live under `$DSH_HOME`. The stub installs nothing silently and imports no harness API.

### From a release

Each release carries a Windows installer, two macOS dmgs (x64 and aarch64), a deb, an rpm, an AppImage and `SHA256SUMS`. The deb depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `libayatana-appindicator3-1`, the last being what the tray needs; on Ubuntu 24.04 and later `libgtk-3-0` is provided by `libgtk-3-0t64`:

```sh
sudo apt install ./dsh-xswt-tauriapp_*_amd64.deb
sudo dnf install ./dsh-xswt-tauriapp-*.x86_64.rpm
```

On Linux the automatic update picks only the `.deb`.

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
| `DSH_HOME` | The DSH home directory; a non-empty value is used as written whether or not it exists yet, and an empty one falls back to `~/.dsh` |
| `DSH_BIN` | The first candidate for dsh's `lib/bin.js`; when it misses, the `$DSH_HOME/profiles`, `npm_config_prefix` and `PATH` candidates are still tried |
| `DSH_NODE_BIN` | The `node` to use; the node that owns the installed dsh takes precedence |
| `DSH_TAURI_REGISTRY` | The version endpoint for dsh; the npm registry by default |
| `DSH_SHELL_RELEASES_API` | The Releases endpoint this application's updater reads; this repository's `releases/latest` by default |
| `DSH_SHELL_ALLOW_MULTIPLE` | Set to let several shells run at once; by default a second launch only raises the one already running |
| `DSH_SHELL_DEBUG` | Set to write the hand-off, navigation and update-check log to stderr |
| `DSH_SHELL_DEVTOOLS` | Set to give a release build its DevTools; unset, neither window has an inspector |
| `DSH_SHELL_ZOOM` | The dsh window's initial zoom factor, which wins over the remembered one; 0.3–3.0 |
| `DSH_SHELL_WAYLAND` | On Linux, set to stop the switch to the X11 backend; an explicit `GDK_BACKEND` is left alone |

Where a row says "set", the test is whether the variable exists, so `0` and an empty value both count. A debug build always writes the log and always has DevTools.

## How it works

| Stage | Behaviour |
|---|---|
| Port band | `3080`–`3129` by default; port candidates come from the file names in `$DSH_HOME/launcher/logs/server-<port>.out.log`, and only logs from the last 90 days count |
| Reuse | `GET /?token=…` returns a 303 with a session cookie, and a second request carrying that cookie checks the page marker; a server without authentication is accepted on a bare `GET /` answering 200 |
| Start | `node <dsh>/lib/bin.js web --port <p> --no-open`, in its own process group, appending to that same log |
| Choosing the port | An adoptable server pre-fills its port, otherwise the default is the first free port of the band and only a typed port is remembered; the prompt lists up to six logged ports with each one's verdict; below `1024` is refused, and a dsh whose session cannot be entered here is named separately |
| Windows | `bootstrap` is a local origin and the only window granted an IPC capability; `dsh` is a remote origin and is granted nothing |
| Hand-off | dsh's session cookie is `SameSite=Strict`, so the handshake and the cookie write finish before the dsh window is built, and its first navigation is started by the host |
| Outbound links | Only `http:`, `https:` and `mailto:` reach the desktop's default handler; `file:` and text that does not parse as a URL are refused |
| Updates | dsh is looked up on npm and this application in its own GitHub Releases; the two run in parallel and neither compares the other's versions; a failed check stays silent; a candidate comes only from a channel at least as stable as the installed one |
| Installer | `SHA256SUMS` is checked before the system sees the file: a mismatch writes nothing, a release without checksums is not downloaded, and a release with nothing installable here offers the release page instead |
| Menu and shortcuts | A shortcut is registered only while a shell window has focus, and zoom goes through the native `set_zoom`; on Linux the X11 backend is used whenever `DISPLAY` exists |
| Remembered | A typed port, the zoom factor and the window's size and position live in the application config directory; a remembered position no display can show is not used |

## Development and verification

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test scripts/changelog.test.mjs
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` has no GUI dependency, so it builds and tests on a machine without `libwebkit2gtk`. `examples/launch.rs` brings a real server up through the same code path the shell uses and prints the session, which is what compat CI uses to verify the handshake; the cookie it prints is a live credential and is redacted before anything reaches a report. `plugins/dsh-desktop-app/` uses Node built-ins only, and its tests run against a stand-in release server on loopback.

## License

[MIT](./LICENSE)
