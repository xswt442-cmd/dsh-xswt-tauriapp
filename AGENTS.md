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
- Discovery stays as dsh has it: token from
  `$DSH_HOME/launcher/logs/server-<port>.out.log`, two-step handshake, detached
  server that outlives the window. The **candidate ports come from those log file
  names**, not from a sweep of 3080–3129: a port with no log has no token, so
  probing it can never pay off, and only the log records a port outside the band.
  `SCAN_CONNECT_TIMEOUT` is for that scan; `CONNECT_TIMEOUT` is for
  `find_free_port`, where a false "free" turns into dsh failing to bind.
- The port is the user's: `discover` offers (and starts nothing), `start_server`
  acts on the answer. Only a port the user **typed** is remembered — accepting the
  suggested default must leave the suggestion free to keep tracking the first free
  port. A port that is listening but not enterable is `Foreign`, not `Occupied`
  (a Windows-side instance seen from WSL looks exactly like that), because
  "occupied" reads as a bug to whoever started it.
- The menu and the tray must never be able to stop startup: `menu::install` is
  logged and ignored on failure. The bootstrap window is the one thing to fail
  hard on, because there is nothing to show without it.
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
- Two update paths, two sources, two dismiss stores: dsh comes from npm
  (`updates`), this application from its own GitHub Releases (`self_update`).
  Never share a store between them, and never compare one product's versions with
  the other's. The self-update path strips a tag's `v` prefix and requires both
  sides to parse as semver, because `updates::is_newer` falls back to string
  inequality — which would offer the running build back as an update. An installer
  is only handed to the OS after its published `SHA256SUMS` matches; a release
  without checksums is refused.
- `plugins/dsh-desktop-app/` is the marketplace stub and is deliberately the
  opposite of everything above: no `@deepseek-ai/*` import, no tool, no client row,
  no window, nothing that can be observed by dsh at all. It imports Node built-ins
  only, so a harness change cannot break it and it cannot become the reason a
  harness fails to boot. It downloads an installer, verifies it against
  `SHA256SUMS` **before writing it**, and hands it to the platform opener — it never
  installs anything and never runs an installer silently, because SmartScreen,
  Gatekeeper and a distribution's root/dependency questions are the platform's to
  ask. Its state is one file under `$DSH_HOME` so the first start does the work and
  every later start stays quiet; the release asset it is fetched through must stay
  version-free, since `releases/latest/download/<name>` takes the filename literally.
- Keep the five version fields equal and each README/CHANGELOG pair in sync. The
  changelog must carry exactly one `## Unreleased` section: `release-notes.mjs`
  takes the first match and would silently drop the rest.
- Icons come from `tauri icon`; changing `src-tauri/icons/` does not rebuild the exe,
  so touch `src-tauri/build.rs` first.
- `web/whale.png` is an unmodified copy of `src-tauri/icons/128x128@2x.png` (only
  the frontend directory is served). The glyph's eye patch and belly are transparent
  cut-outs, so they read as background-coloured voids: measured, they survive at 64
  and 96 device pixels (2x displays from ~32px) but not at 32-48 on a 1x display,
  where the mark is a soft pink shape. That is accepted, not compensated for by
  editing the brand asset — the mark is `aria-hidden` decoration and the product name
  sits beside it in text. Raising the size further stops helping long before it fixes
  a 1x display.

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

- `npm run build` bundles whatever `tauri.conf.json` declares, so it is valid on
  every platform; the first Windows run needs the NSIS/WiX toolchain from
  github.com, cached under `%LOCALAPPDATA%\tauri`.
- `compat.yml` compiles the harness on Windows and macOS, but compiling catches
  types, not behaviour — no CI job runs the GUI.
- `cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` walks the
  real handshake and prints `url=` and `cookie=`. That stdout is a **live session
  cookie**: redact it.
- `cargo run --example port-choice --manifest-path crates/dsh-core/Cargo.toml`
  walks `plan` → `check_port` → `start_on` without a GUI, including the custom-port
  case and the `Occupied`/`Foreign` distinction. It leaves its servers running.
- The hand-off is measured on Linux and Windows and **not** on macOS. The Windows
  recipe, and everything else learned there, is in `testplace/WORKLOG.md`.
