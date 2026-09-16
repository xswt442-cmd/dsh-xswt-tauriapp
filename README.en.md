# dsh-xswt-tauriapp

[中文](./README.md) | [English](./README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)
![DSH shell](https://img.shields.io/badge/DSH-shell-4d6bfe)

A lightweight Tauri desktop shell for DeepSeek Harness. It reuses or starts a local `dsh web` server, embeds the interface in a native window, and checks for dsh updates on launch. It does not modify dsh, and it does not take part in dsh's session, sandbox or permission model.

## Features

- **Reuse or start a server**: scans ports 3080–3129 and reuses a running `dsh web` when there is one; otherwise it starts one on the first free port.
- **Launch-token handshake**: dsh hands its interface only to a request carrying the token it minted at startup. The shell reads that token from the server log and completes the two-step handshake, so an unrelated service on the port is never mistaken for dsh.
- **Update check on launch**: reads the published versions of `@deepseek-ai/dsh` from npm and shows them in three columns — stable, RC and alpha — with the option to install any of them and restart.
- **Don't remind me about this version**: suppresses the launch prompt for that one version; a newer version still prompts.
- **External links open in the browser**: links that leave the app are handed to the desktop's default handler instead of taking over the window.
- **Reload is kept available**: the window has no browser chrome, so `Ctrl+R` / `F5` reloads. Some dsh settings — the content font size and the theme's boot values — are written when the host renders the index, so they need a reload to take effect.
- **The server outlives the shell**: it runs in its own process group, and closing the window never stops it.

## Getting it

### From a release

Each release carries a Windows installer, a macOS dmg, a deb, an rpm and an AppImage,
together with `SHA256SUMS`. The deb depends only on `libwebkit2gtk-4.1-0` and `libgtk-3-0`:

```sh
sudo apt install "./dsh-xswt-tauriapp_0.1.0_amd64.deb"
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

The shell shows its own page first while it discovers the server and checks for updates, then hands the window over to the dsh interface. Environment variables:

| Variable | Effect |
|---|---|
| `DSH_HOME` | DSH home directory; defaults to `~/.dsh` |
| `DSH_BIN` | Points directly at dsh's `lib/bin.js` |
| `DSH_NODE_BIN` | Points directly at the `node` executable |
| `DSH_TAURI_REGISTRY` | Overrides the version endpoint; defaults to the npm registry |
| `DSH_SHELL_DEBUG` | When non-empty, logs navigation and update checks |

## How it works

### Server discovery and handshake

| Step | Behaviour |
|---|---|
| Port band | `3080`–`3129` |
| Reuse test | Reads the launch token from the tail of `$DSH_HOME/launcher/logs/server-<port>.out.log` |
| Handshake | `GET /?token=…` → 303 with a session cookie → re-request with the cookie, then check the page for `DeepSeek Harness` |
| Unauthenticated server | A bare `GET /` answering 200 with the marker is accepted too |
| Start | `node <dsh>/lib/bin.js web --port <p> --no-open`, in its own process group, appending to that same log |

The server log is the only source of the token, so an instance started by hand in a terminal — whose log never lands there — is not recognised, and the shell starts one of its own. That follows from dsh's authentication model; the shell does not guess around it.

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
| Linux (deb / rpm / AppImage) | Supported; produced by CI |
| Windows (NSIS installer) | Build wired up; the core logic is verified on `windows-latest` by CI, the GUI has not been exercised |
| macOS (dmg) | Build wired up, unsigned; the GUI has not been exercised |
| WSLg | Runs; WebKitGTK's GPU passthrough is unreliable, so set `WEBKIT_DISABLE_COMPOSITING_MODE=1` and `WEBKIT_DISABLE_DMABUF_RENDERER=1` |

## Development and verification

```sh
cargo test --manifest-path crates/dsh-core/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
node scripts/check-docs.mjs
cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml
```

`crates/dsh-core` has no GUI dependency, so it builds and tests on a machine without `libwebkit2gtk`. `examples/launch.rs` brings a real server up through the same code path the shell uses and prints its URL.

## License

[MIT](./LICENSE)
