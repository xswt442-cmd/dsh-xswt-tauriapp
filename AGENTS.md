# Agent guide

`dsh-xswt-tauriapp` is a Tauri **harness** for DeepSeek Harness. Tauri owns
dsh's periphery — windows, the server process, launching and reuse, the session
hand-off, menus/tray/shortcuts, updates, external links, failure recovery — and
dsh owns its own page. It does not modify, patch or vendor dsh.

## Workflow

- Develop on `dev`; keep `main` release-only.
- Lowercase Conventional Commit prefixes. Never use `--no-verify`.
- `testplace/` is ignored scratch — notes go there, not into tracked source.
- Read `RELEASING.md` only when preparing a release.

## Engineering

- **The rule that decides where code goes:** if it does not need to understand
  dsh's DOM, the harness should do it; if it needs to read or modify dsh's DOM,
  it should not be done. No injected scripts, no DOM reads, no CSS patches, no
  shell UI drawn over dsh's window.
- Two windows, and the split is load-bearing: `bootstrap` (local origin, the only
  one with a capability) carries progress, updates and failures; `dsh` (remote
  origin, no capability) carries the dsh interface alone.
- Keep dsh's security semantics. Its session cookie is `SameSite=Strict`; never
  relax it. The first navigation into dsh must be **host-initiated**, with the
  cookie already in the store — that is what makes Strict work, and a shell page
  navigating to dsh is what breaks it.
- `set_cookie` is asynchronous and `cookies_for_url` is a blocking getter that
  deadlocks on Windows from the main thread. Cookie work belongs on a worker
  thread; window building belongs on the main thread.
- Non-GUI logic belongs in `crates/dsh-core`, which must keep building and
  testing without webkit2gtk. It returns a prepared `Session` (clean URL + cookie),
  never a token URL — the launch token must not reach a page.
- Discovery stays as dsh has it: ports 3080–3129, token from
  `$DSH_HOME/launcher/logs/server-<port>.out.log`, two-step handshake, detached
  server that is never stopped when the window closes.
- Tauri can only bind a shortcut through a menu accelerator, and a window menu is
  a visible menu bar on Windows/Linux. Those platforms use a tray plus global
  shortcuts held **only while one of our windows has focus**. Never hold them
  permanently. If the desktop refuses them, the app must still start.
- `global-hotkey`'s manager is **not `Send`/`Sync` on Windows** (a bare `HWND`),
  so it can never be Tauri managed state — a `Mutex` would not help either, since
  `Mutex<T>: Sync` needs `T: Send`. Only plain data goes into managed state; the
  manager lives in a `thread_local`, which is also where both platforms require
  it (Windows needs its creator's message loop, macOS wants the main thread). The
  *event* handler may run on another thread, so it must reach the bindings
  through managed state and never through the `thread_local`.
- The Linux backend is load-bearing, not cosmetic: `global-hotkey` grabs through
  X11, and a Wayland-native window's keys never reach the X server, so shortcuts
  register and then never fire. `prefer_x11()` switches to X11 whenever `DISPLAY`
  exists; keep the `DSH_SHELL_WAYLAND` opt-out working.
- A tray icon needs a StatusNotifier host. WSLg has none, so the icon is created
  and simply never shown — the environment, not the code.
- Do not use `zoom_hotkeys_enabled`: on macOS and Linux it works by injecting a
  polyfill into the page. Zoom goes through `set_zoom` from Rust.
- Never offer an update from a channel less stable than the installed one, and
  keep "don't remind me" scoped to one version.
- Keep every declared version equal — both crates, `package.json` and
  `tauri.conf.json` — and the README/CHANGELOG pairs in sync.

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

The harness itself is only compiled on Linux by `cargo check` in this list, so a
platform-specific mismatch would reach a tag — that is how `global-hotkey`'s
non-`Send` Windows manager got as far as bundling. `compat.yml` therefore checks
it on `windows-latest` and `macos-latest` too, and a Windows/macOS type can be
checked locally without a cross linker:

```sh
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
```

(`check` does not link, so no MSVC toolchain is needed. Dependencies that build C
code — `ring`, through `ureq` — may still refuse to cross-compile; a scratch crate
depending only on the suspect crate is enough to type-check a platform type.)

`cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` boots a
real server through the shell's own code path and prints its prepared session
(`url=` and `cookie=`).

The hand-off's central claim — that a host-initiated first navigation carries the
`SameSite=Strict` cookie — is measured on Linux/WebKitGTK and unmeasured on
Windows and macOS. Treat it as unverified there.

Whether a synthesised X11 key reaches a root-window grab could **not** be measured
under WSLg: `XTEST` events never arrived, with `global-hotkey` in isolation, which
makes that a limitation of the environment rather than evidence about the code.
The steps up to it are verified (the app runs as an X11 client, all five grabs
register, focus transitions fire).
