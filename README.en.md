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

A lightweight Tauri desktop shell for DeepSeek Harness, and a **desktop harness / runtime supervisor** for dsh: Tauri owns dsh's periphery — windows, the server process, launching and reuse, the session hand-off, menus and tray, shortcuts, updates, external links and failure recovery — while dsh owns its own page. The shell does not modify dsh: it does not inject a script into dsh's page, read its DOM, patch its styles or draw shell UI over it, and it takes no part in dsh's session, sandbox or permission model.

## Features

- **Reuse or start a server**: reuses a running `dsh web` in `3080`–`3129`, or starts one on the first free port; the server process is independent of the shell, so closing the window never stops it.
- **Launch-token handshake and session hand-off**: the handshake is walked and verified in Rust, and the launch token never reaches a page's URL.
- **Pick the port at launch**: the default comes from the scan or from the port typed last, and any other port can be typed; availability is answered on the spot.
- **Update prompts**: dsh's updates are shown in stable / RC / alpha columns, while this application's own updates pick and checksum the installer for this platform; each keeps its own "don't remind me" record.
- **System integration**: a native system menu on macOS, a tray menu on Windows and Linux; `Ctrl/Cmd+R`, `Ctrl/Cmd+=` `-` `0` and `F12` are registered only while one of the shell's windows has focus.
- **Links that leave the app** are handed to the desktop's default handler instead of taking over the window.

## Platform and compatibility

| Item | Status |
|---|---|
| Linux (deb / rpm / AppImage) | Supported; produced by CI. The session hand-off and first navigation are exercised |
| Windows (NSIS installer) | Supported; the installer and GUI are exercised in daily use, and the hand-off and first navigation have been measured |
| macOS (dmg) | Build wired up, unsigned; the GUI has not been exercised |
| WSLg | Runs; WebKitGTK's GPU passthrough is unreliable, so set `WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1`. WSLg has no status-bar host, so a tray icon has nowhere to appear; shortcuts work, provided the X11 backend is used. WSLg also renders at scale 1 whatever the Windows display scale is, so on a 125% or 150% display its windows are smaller than native ones: zoom with `Ctrl/Cmd+=`, which is remembered, or pin `DSH_SHELL_ZOOM` |

That the first navigation carries the `SameSite=Strict` cookie has been measured on Linux / WebKitGTK and on Windows / WebView2; macOS relies on its webview treating an initiator-less navigation as same-site, which has not been measured.

## Getting it

### From the plugin market

The [marketplace](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin) entry points at `dsh-xswt-tauriapp-plugin.tgz`, which every release of this repository carries. It is an installer stub rather than the application itself (source: [`plugins/dsh-desktop-app/`](plugins/dsh-desktop-app/README.md)):

```sh
dsh plugin --profile web add https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

On the **first** dsh start after installing it, the stub reads this repository's latest release, picks the installer for this platform, fetches `SHA256SUMS` and only writes the file once its digest matches, then hands it to the system installer. A machine that already has the shell, or one with no desktop session (CI, or Linux without `DISPLAY`), is only told where to look. It installs nothing silently and imports no harness API, so it cannot become the reason a harness fails to start.

### From a release

Each release carries a Windows installer, a macOS dmg, a deb, an rpm and an AppImage, together with `SHA256SUMS`. The deb depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `libayatana-appindicator3-1` (the tray; on Ubuntu 24.04 and later GTK3 ships as `libgtk-3-0t64`, which `Provides: libgtk-3-0`, so it installs either way):

```sh
sudo apt install "./dsh-xswt-tauriapp_0.0.12_amd64.deb"
```

### From source

Rust, Node and Tauri's Linux system dependencies are required:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev rpm
pnpm install
pnpm tauri build --bundles deb,rpm,appimage
```

## Usage

```sh
./src-tauri/target/release/dsh-xswt-tauriapp
```

The shell shows the `bootstrap` page first while it discovers the server and checks for updates, then opens the dsh window. Environment variables:

| Variable | Effect |
|---|---|
| `DSH_HOME` | DSH home directory; defaults to `~/.dsh` |
| `DSH_BIN` | Points directly at dsh's `lib/bin.js` |
| `DSH_NODE_BIN` | Points directly at the `node` executable |
| `DSH_TAURI_REGISTRY` | Overrides the version endpoint; defaults to the npm registry |
| `DSH_SHELL_ALLOW_MULTIPLE` | When non-empty, several shells may run at once; by default a second launch only raises the one already running |
| `DSH_SHELL_DEBUG` | When non-empty, logs the hand-off, navigation and update checks |
| `DSH_SHELL_DEVTOOLS` | When non-empty, offers DevTools in the menu (debug builds already do) |
| `DSH_SHELL_ZOOM` | The dsh window's initial zoom factor (0.3–3.0), which wins over the remembered one |
| `DSH_SHELL_WAYLAND` | On Linux, do not switch to the X11 backend (see "Menus, tray and shortcuts") |

## How it works

### Server discovery, handshake and session hand-off

| Step | Behaviour |
|---|---|
| Port band | `3080`–`3129` |
| Reuse test | Reads the launch token from the tail of `$DSH_HOME/launcher/logs/server-<port>.out.log` |
| Handshake (in Rust) | `GET /?token=…` → 303 with a session cookie → re-request with the cookie, then check the page for `DeepSeek Harness` |
| Hand-off | Writes the cookie into the cookie store — supplying the `Domain` the server omitted — and only builds the dsh window once it reads back |
| Unauthenticated server | A bare `GET /` answering 200 with the marker is accepted too |
| Start | `node <dsh>/lib/bin.js web --port <p> --no-open`, in its own process group, appending to that same log |

The server log is the only source of the token: an instance started by hand in a terminal, whose log never lands there, is not recognised, and the shell starts one of its own. That follows from dsh's authentication model; the shell does not guess around it.

### Choosing the port

| Situation | Behaviour |
|---|---|
| An adoptable server was found | The field is pre-filled with its port; confirming reuses it. Typing another port starts a second instance |
| Nothing adoptable | Default is the first free port of `3080`–`3129`, or a port typed before while it is still free |
| Another program owns the port | Said there and then; confirming does not go ahead |
| A dsh is there that this machine cannot enter | Called out separately — a Windows-side instance, or one started under a different `DSH_HOME` |
| Below `1024` | Refused: an ordinary user cannot bind it |

Only a port that was **typed** is remembered, and used as the next default. Accepting the grey default is not a choice, so the default keeps tracking the first free port.

Discovery candidates come from the **file names** in `$DSH_HOME/launcher/logs/server-<port>.out.log` rather than from a sweep of the whole band: a port with no log has no launch token, so its handshake could never complete and probing it is wasted work. That is also what lets a deliberately unusual port — `9000`, say — be found again on the next launch.

### Window model and the first navigation

| Window | Content | Access |
|---|---|---|
| `bootstrap` | The shell's own page: progress, the update dialog, failure text | A local origin, and the only window granted an IPC capability |
| `dsh` | dsh's own interface, untouched | A remote origin, granted nothing |

dsh's session cookie is `SameSite=Strict`, and a navigation started by a page at another origin does not carry it — which is why navigating the shell page to dsh stops on dsh's 401 text. The shell keeps dsh's security semantics and changes the order instead: Rust completes the handshake and writes the cookie into the cookie store first, and only then builds the dsh window on `http://127.0.0.1:<port>/`. That window's first navigation is started by the host with no initiating page, so it is not a cross-site request.

### Menus, tray and shortcuts

| Platform | Shape |
|---|---|
| macOS | A native system menu, including an Edit menu — the standard text shortcuts depend on it |
| Windows / Linux | A tray menu; no permanent menu bar, so none of dsh's height is spent on one |

Tauri can bind a keyboard shortcut only through a menu accelerator, and on Windows and Linux a menu attached to a window *is* a visible menu bar. Those platforms therefore use global shortcuts, registered only while one of the shell's windows has focus and released on blur, so `Ctrl+R` is not taken away from the rest of the machine. If the desktop refuses to hand out global shortcuts, the shell still starts; it just loses the gestures.

On Linux there is one further condition: `global-hotkey` grabs keys through X11, and a Wayland-native window's keystrokes never pass through the X server, so the shortcuts register successfully and then never fire. The shell therefore uses the X11 backend whenever `DISPLAY` exists (XWayland is present on every Wayland desktop); `DSH_SHELL_WAYLAND=1` opts out, and an explicit `GDK_BACKEND` is left alone.

Zoom is applied from Rust through the native `set_zoom`: the webview's own zoom hotkeys work by injecting a polyfill into the page on macOS and Linux, which conflicts with the no-injection rule. The factor is remembered in the application config directory, so it survives a restart, and `DSH_SHELL_ZOOM` seeds it for a session — the environment wins.

The dsh window's size and position are remembered too, in `window.json` beside it, and restored on the next launch. A remembered position is used only while a display that is here *now* can still show it: unplug the monitor it was recorded on and those coordinates are off-screen, so restoring them would mean launching a window nobody can see, and the window centres instead. Launching the shell again does not open a second window either — the second process raises the one already running (`DSH_SHELL_ALLOW_MULTIPLE=1` opts out, for two shells on two ports side by side), and on macOS reactivating the app from the Dock does the same.

### Updates to this application itself

Two independent paths: dsh comes from npm, this application comes from its own GitHub Releases. Startup asks both at once, and neither waits for the other.

| Situation | Behaviour |
|---|---|
| A newer build with an installer for this machine | The dialog offers *Download and install*; the file is checked against `SHA256SUMS` and only then handed to the system installer |
| A newer build with nothing installable here | The action becomes *Open the release page* — no guessing, nothing else downloaded |
| The check fails (offline, rate-limited API, no release yet) | Silent. Not knowing about an update is not a reason to interrupt anyone |
| The checksum does not match, or the release publishes none | Refused, with the expected and actual hashes named; nothing is written |

Both sides must parse as semver and the tag's `v` prefix is stripped first. The dsh comparator is not reused here: its string fallback suits a version feed, while an updater using it would offer the build the user is already running. "Don't remind me" is recorded per application version in its own file (`dismissed-shell-updates.json`), so it can neither silence a dsh update nor be silenced by one.

### Updates to dsh itself

Channels are derived from the version string rather than from npm's dist-tags, because a tag can itself point at a release candidate.

| Channel | Test |
|---|---|
| Stable | No prerelease suffix, e.g. `0.1.5` |
| RC | Prerelease starts with `rc`, e.g. `0.1.5-rc.2` |
| Alpha | Prerelease starts with `alpha`, e.g. `0.1.6-alpha.1` |

The launch prompt only offers a candidate from a channel at least as stable as the installed one: an RC install is offered RC or stable, and alpha has to be chosen deliberately from the dialog. "Don't remind me about this version" records that version in the app config directory and suppresses only that version.

## Development and verification

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node --test plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` has no GUI dependency, so it builds and tests on a machine without `libwebkit2gtk`. `examples/launch.rs` brings a real server up through the same code path the shell uses and prints the prepared session (`url=` and `cookie=`); compat CI uses it to verify the handshake and to pin the fact that the token stays out of the URL.

The marketplace stub in `plugins/dsh-desktop-app/` is a dependency-free Node module, and every case in its suite runs against a stand-in release server on loopback: nothing reaches GitHub, and `DSH_TAURIAPP_NO_OPEN=1` stops each run at the point where it would have handed the file over.

## License

[MIT](./LICENSE)
