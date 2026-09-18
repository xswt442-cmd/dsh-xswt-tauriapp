# Agent guide

`dsh-xswt-tauriapp` is a Tauri harness for DeepSeek Harness: Tauri owns dsh's
periphery, dsh owns its own page. It never patches or vendors dsh.

## Workflow

- Develop on `dev`; keep `main` release-only, and `main` is the repository's default
  branch — so `HEAD` readers (the marketplace screenshot probe, the release-notes step)
  follow it, not `dev`.
- Lowercase Conventional Commit prefixes. Never use `--no-verify`.
- `testplace/` is ignored scratch — records, measurements, long explanations — with
  `WORKLOG.md` as the running log. Keep this file short.
- Read `RELEASING.md` before a release.

## Engineering

- Nothing that needs to understand dsh's DOM belongs here: no injected scripts, no DOM
  reads, no CSS patches, no shell UI over dsh's window.
- Two windows, and the split is load-bearing: `bootstrap` (local origin, sole capability)
  carries progress, updates and failures; `dsh` (remote, no capability) shows dsh alone.
  Failures also reach the shell's log through `page_diag` — stderr in debug builds, and in
  a release only with `DSH_SHELL_DEBUG` set, since a packaged GUI has no terminal.
- The hand-off's mechanics are load-bearing: `SameSite=Strict` stays, and the first
  navigation into dsh is host-initiated with the cookie already stored; cookie work runs
  on a worker thread (`cookies_for_url` deadlocks on Windows from the main thread); a
  window-building command must be `async`, or it deadlocks in the webview's IPC callback
  on Windows (wry#583).
- Non-GUI logic lives in `crates/dsh-core`: it builds and tests without webkit2gtk, and
  hands over a prepared `Session` (clean URL + cookie), never a token URL.
- Discovery stays as dsh has it — token from
  `$DSH_HOME/launcher/logs/server-<port>.out.log`, two-step handshake, detached server
  that outlives the window. **Candidate ports come from those log file names**, not from a
  sweep of 3080–3129: no log means no token, and only a log records a port outside the
  band. `SCAN_CONNECT_TIMEOUT` scans; `CONNECT_TIMEOUT` is `find_free_port`, where a false
  "free" becomes dsh failing to bind.
- The port is the user's: `discover` offers and starts nothing, `start_server` acts on the
  answer. Only a **typed** port is remembered — accepting the default must leave the
  suggestion free to keep tracking the first free port. Listening but not enterable is
  `Foreign`, not `Occupied` (a Windows-side instance seen from WSL looks like that;
  "occupied" reads as a bug).
- Menu and tray must never be able to stop startup: `menu::install` is logged and ignored
  on failure. The bootstrap window is the one thing to fail hard on.
- Shortcuts are tray-based and grabbed **only while one of our windows has focus**; a
  refused grab is normal (bare `F12` often is), so release each binding on its own —
  `unregister_all` stops at the first failure. `global-hotkey`'s manager is not
  `Send`/`Sync` on Windows: `thread_local`, never managed state.
- Linux takes the X11 backend whenever `DISPLAY` exists (`prefer_x11`) — Wayland-native
  keys never reach X11's grabs; `DSH_SHELL_WAYLAND=1` opts out. A tray icon needs a
  StatusNotifier host, which WSLg lacks.
- No `zoom_hotkeys_enabled` (it injects a polyfill): zoom goes through `set_zoom`.
- Never offer an update from a channel less stable than the installed one, and scope
  "don't remind me" to one version.
- Two update paths, two stores: dsh from npm (`updates`), this application from its own
  GitHub Releases (`self_update`). Never share a store or compare one product's versions
  with the other's. Self-update strips the tag's `v` and requires semver on both sides,
  because `updates::is_newer` falls back to string inequality. An installer reaches the OS
  only after its published `SHA256SUMS` matches; no checksums, no install.
- `plugins/dsh-desktop-app/` is the marketplace stub and deliberately the opposite of
  everything above: Node built-ins only, no `@deepseek-ai/*`, no tool, no client row, no
  window, nothing dsh can observe. It verifies against `SHA256SUMS` **before writing** and
  hands the file to the platform opener — never installing, never silently, because
  SmartScreen, Gatekeeper and dependency/root questions are the platform's to ask. Its
  state is one file under `$DSH_HOME`; the asset it is fetched through must stay
  version-free (`releases/latest/download/<name>` is literal).
- Keep the five version fields equal and each README/CHANGELOG pair in sync, with exactly
  one `## Unreleased` section: `release-notes.mjs` takes the first match. A section is the
  release body verbatim, so entries stay one line and technical — the debugging story goes
  in the commit or `testplace/WORKLOG.md`.
- Icons come from `tauri icon`; changing `src-tauri/icons/` alone does not rebuild the exe,
  so touch `src-tauri/build.rs` first.
- `web/whale.png` is a byte copy of `src-tauri/icons/128x128@2x.png`; its transparent
  cut-outs read as background voids — fine at 64px and up, not at 32–48 on a 1x display.
  Accepted: it is `aria-hidden` decoration beside the name in text.

## Verify

```sh
cargo test   --manifest-path crates/dsh-core/Cargo.toml
cargo test   --manifest-path src-tauri/Cargo.toml --lib
cargo fmt    --manifest-path crates/dsh-core/Cargo.toml --check
cargo fmt    --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path crates/dsh-core/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
node --test  plugins/dsh-desktop-app/test/plugin.test.js
node scripts/check-docs.mjs
```

- `npm run build` bundles what `tauri.conf.json` declares, so it is valid everywhere; the
  first Windows run downloads the NSIS/WiX toolchain into `%LOCALAPPDATA%\tauri`.
- `compat.yml` compiles on Windows and macOS, but types are not behaviour: no CI job runs
  the GUI.
- `cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` walks the real
  handshake and prints `url=` and `cookie=` — a **live session cookie**, so redact it.
- `cargo run --example port-choice --manifest-path crates/dsh-core/Cargo.toml` walks
  `plan` → `check_port` → `start_on` without a GUI (custom port, `Occupied`/`Foreign`) and
  leaves its servers running.
- The hand-off is measured on Linux and Windows, **not** macOS; the Windows recipe is in
  `testplace/WORKLOG.md`.
