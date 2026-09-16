# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
![DSH shell](https://img.shields.io/badge/DSH-shell-4d6bfe)

A lightweight Tauri desktop shell for DeepSeek Harness — more precisely, a **desktop harness / runtime supervisor** for dsh. Tauri owns everything around it: windows, the server process, launching and reuse, the session hand-off, menus/tray/shortcuts, updates, external links and failure recovery; dsh owns its own page. The shell does not modify dsh, and it **does not inject a script into dsh's page, read its DOM, patch its CSS, or draw shell UI over it** — nor does it take part in dsh's session, sandbox or permission model.

## Features

- **Reuse or start a server**: scans ports 3080–3129 and reuses a running `dsh web` when there is one; otherwise it starts one on the first free port.
- **Launch-token handshake and session hand-off**: dsh hands its interface only to a request carrying the token it minted at startup. The handshake is walked and verified in Rust, and the resulting session cookie is given to the window — the launch token never reaches a page's URL.
- **Two windows**: the shell's own `bootstrap` page carries progress, updates and failures; the `dsh` window shows dsh alone. Nothing from startup can end up drawn over dsh.
- **Update check on launch**: reads the published versions of `@deepseek-ai/dsh` from npm and shows them in three columns — stable, RC and alpha — with the option to install any of them and restart.
- **Don't remind me about this version**: suppresses the launch prompt for that one version; a newer version still prompts.
- **Menus, tray and shortcuts**: macOS uses a native system menu; Windows and Linux use a tray menu, with no permanent menu bar. `Ctrl/Cmd+R` reloads, `Ctrl/Cmd+=` `-` `0` zoom, `F12` opens DevTools.
- **External links open in the browser**: links that leave the app are handed to the desktop's default handler instead of taking over the window.
- **The server outlives the shell**: it runs in its own process group, and closing the window never stops it.

## Getting it

### From a release

Each release carries a Windows installer, a macOS dmg, a deb, an rpm and an AppImage,
together with `SHA256SUMS`. The deb depends only on `libwebkit2gtk-4.1-0` and `libgtk-3-0`:

```sh
sudo apt install "./dsh-xswt-tauriapp_0.0.4_amd64.deb"
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
| `DSH_SHELL_DEBUG` | When non-empty, logs the hand-off, navigation and update checks |
| `DSH_SHELL_DEVTOOLS` | When non-empty, offers DevTools in the menu (debug builds already do) |

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

The server log is the only source of the token, so an instance started by hand in a terminal — whose log never lands there — is not recognised, and the shell starts one of its own. That follows from dsh's authentication model; the shell does not guess around it.

### Window model and the first navigation

| Window | Content | Access |
|---|---|---|
| `bootstrap` | The shell's own page: progress, the update dialog, failure text | A local origin, and the only window granted an IPC capability |
| `dsh` | dsh's own interface, untouched | A remote origin, granted nothing |

dsh's session cookie is `SameSite=Strict`. A navigation started by a page at another origin does not carry it — which is exactly why navigating the *shell page* to dsh used to stop on dsh's 401 text. The shell keeps dsh's security semantics and changes the order instead: Rust completes the handshake and writes the cookie into the cookie store **first**, and only then builds the dsh window on `http://127.0.0.1:<port>/`. That window's first navigation is started by the host with no initiating page, so it is not a cross-site request and the Strict cookie is sent.

### Menus, tray and shortcuts

| Platform | Shape |
|---|---|
| macOS | A native system menu, including an Edit menu — the standard text shortcuts depend on it |
| Windows / Linux | A tray menu; no permanent menu bar, so none of dsh's height is spent on one |

Tauri can only bind a keyboard shortcut through a menu accelerator, and on Windows and Linux a menu attached to a window *is* a visible menu bar. So on those platforms the shortcuts are global shortcuts, registered **only while one of the shell's windows has focus** and released on blur — `Ctrl+R` is not taken away from the rest of the machine. If the desktop refuses to hand out global shortcuts, the shell still starts; it just loses the gestures.

Zoom is applied from Rust through the native `set_zoom`: the webview's own zoom hotkeys work by injecting a polyfill into the page on macOS and Linux, which would conflict with the no-injection rule.

### Update checks

Channels are derived from the version string rather than from npm's dist-tags, because a tag can itself point at a release candidate.

| Channel | Test |
|---|---|
| Stable | No prerelease suffix, e.g. `0.1.5` |
| RC | Prerelease starts with `rc`, e.g. `0.1.5-rc.2` |
| Alpha | Prerelease starts with `alpha`, e.g. `0.1.6-alpha.1` |

The launch prompt only offers a candidate from a channel at least as stable as the installed one: an RC install is offered RC or stable, and alpha has to be chosen deliberately from the dialog. "Don't remind me about this version" records that version in the app config directory and suppresses only that version.

## Platform and compatibility

| Item | Status |
|---|---|
| Linux (deb / rpm / AppImage) | Supported; produced by CI. The session hand-off and first navigation are exercised |
| Windows (NSIS installer) | Build wired up; the core logic is verified on `windows-latest` by CI, the GUI has not been exercised |
| macOS (dmg) | Build wired up, unsigned; the GUI has not been exercised |
| WSLg | Runs; WebKitGTK's GPU passthrough is unreliable, so set `WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1` |

That the first navigation carries the `SameSite=Strict` cookie has been measured on Linux / WebKitGTK. Windows and macOS rely on their own webviews treating an initiator-less navigation as same-site, which has not been measured.

## Development and verification

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` has no GUI dependency, so it builds and tests on a machine without `libwebkit2gtk`. `examples/launch.rs` brings a real server up through the same code path the shell uses and prints the prepared session (`url=` and `cookie=`); compat CI uses it to verify the handshake and to pin the fact that the token stays out of the URL.

## License

[MIT](./LICENSE)
