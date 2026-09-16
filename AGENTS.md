# Agent guide

`dsh-xswt-tauriapp` is a Tauri harness for DeepSeek Harness: Tauri owns dsh's
periphery, dsh owns its own page. It never patches or vendors dsh.

## Workflow

- Develop on `dev`; keep `main` release-only.
- Lowercase Conventional Commit prefixes. Never use `--no-verify`.
- `testplace/` is ignored scratch: investigation records, measurements and long
  explanations go there, with `WORKLOG.md` as the running log. Keep this file short.
- Read `RELEASING.md` when preparing a release.

## Engineering

- If it needs to understand dsh's DOM it does not belong here: no injected scripts,
  no DOM reads, no CSS patches, no shell UI over dsh's window.
- Two windows, and the split is load-bearing: `bootstrap` (local origin, the only
  one with a capability) carries progress, updates and failures; `dsh` (remote
  origin, no capability) shows dsh alone. Failures reach stderr through `page_diag`,
  not just the failure view — a packaged GUI has no terminal.
- The hand-off's mechanics are all load-bearing: the cookie stays `SameSite=Strict`
  and the first navigation into dsh is host-initiated with the cookie already in the
  store; cookie work runs on a worker thread because `cookies_for_url` deadlocks on
  Windows from the main thread; and a command that builds a window must be `async`,
  since a synchronous one runs inside the webview's IPC callback and deadlocks on
  Windows (wry#583).
- Non-GUI logic lives in `crates/dsh-core`, which must build and test without
  webkit2gtk, and hands over a prepared `Session` (clean URL + cookie) — never a
  token URL.
- Discovery stays as dsh has it: ports 3080–3129, token from
  `$DSH_HOME/launcher/logs/server-<port>.out.log`, two-step handshake, detached
  server that outlives the window.
- Shortcuts are tray-based and grabbed **only while one of our windows has focus**,
  never permanently. A refused grab is normal — bare `F12` often is taken — so the
  release path must tolerate it: release each binding on its own, because
  `unregister_all` stops at the first failure. `global-hotkey`'s manager is not
  `Send`/`Sync` on Windows, so it lives in a `thread_local`, never in managed state.
- Linux takes the X11 backend whenever `DISPLAY` exists (`prefer_x11`), because a
  Wayland-native window's keys never reach X11's grabs; `DSH_SHELL_WAYLAND=1` opts
  out. A tray icon needs a StatusNotifier host, and WSLg has none.
- No `zoom_hotkeys_enabled` (it injects a polyfill); zoom goes through `set_zoom`.
- Never offer an update from a channel less stable than the installed one, and scope
  "don't remind me" to one version.
- Keep the four version fields equal and each README/CHANGELOG pair in sync.
- Icons come from `tauri icon`; changing `src-tauri/icons/` does not rebuild the exe,
  so touch `src-tauri/build.rs` first.

## Verify

```sh
cargo test   --manifest-path crates/dsh-core/Cargo.toml
cargo test   --manifest-path src-tauri/Cargo.toml --lib
cargo fmt    --manifest-path crates/dsh-core/Cargo.toml --check
cargo fmt    --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path crates/dsh-core/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
node scripts/check-docs.mjs
```

- `npm run build` bundles whatever `tauri.conf.json` declares, so it is valid on
  every platform; the first Windows run needs the NSIS/WiX toolchain from
  github.com, cached under `%LOCALAPPDATA%\tauri`.
- `compat.yml` compiles the harness on Windows and macOS, but compiling catches
  types, not behaviour — no CI job runs the GUI.
- `cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` walks the
  real handshake and prints `url=` and `cookie=`. That stdout is a **live session
  cookie**: redact it.
- The hand-off is measured on Linux and Windows and **not** on macOS. The Windows
  recipe, and everything else learned there, is in `testplace/WORKLOG.md`.
