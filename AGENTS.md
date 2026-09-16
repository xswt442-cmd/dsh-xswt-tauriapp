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
  thread.
- A command that builds a window must be `async`. A synchronous one runs inside
  the webview's own IPC callback, and building a second webview from there
  deadlocks on Windows (wry#583): the window is created and never handed back, so
  the guest stays hidden behind the splash and dsh is never reached. The window
  is still *built* on the main thread, which is where Tauri dispatches it.
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
- A refused grab is normal, not an error: another application may already own the
  key, and bare `F12` commonly does. A failed registration is therefore logged and
  skipped. The release path has to tolerate it the same way — release each binding
  on its own, because `GlobalHotKeyManager::unregister_all` stops at the first key
  it cannot release and would leave every key after it held for good.
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

`npm run build` bundles whatever `tauri.conf.json` declares (`targets: "all"`)
rather than naming one platform's list, so it is valid everywhere. The first
Windows run downloads the NSIS/WiX toolchain from github.com into
`%LOCALAPPDATA%\tauri`; a host that cannot reach github.com stalls at
`Verifying wix package` until that cache is seeded.

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

Compiling elsewhere catches types, not behaviour: the 0.0.5 hand-off deadlock
type-checked and tested clean on all three platforms and still left the app
unusable on Windows. No CI job runs the GUI, so the runtime half of the harness is
verified by hand, on the platform it ships to.

`cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` boots a
real server through the shell's own code path and prints its prepared session
(`url=` and `cookie=`). That stdout is a **live session cookie**: redact the value
before it reaches a log, a commit or a transcript. Its name is minted per
handshake (`dsh-auth-<random>`), so nothing may match on a fixed name — the
harness parses whatever `Set-Cookie` the server sends.

The hand-off's central claim — that a host-initiated first navigation carries the
`SameSite=Strict` cookie — is measured on Linux/WebKitGTK and on Windows/WebView2,
and unmeasured on macOS. Treat it as unverified there only.

On Windows, run the release build with `DSH_SHELL_DEBUG=1` and read the hand-off
in order: `the session cookie is in the jar` (a write that was read back, so the
guest navigated with the cookie already in a shared store), `guest window
created`, `guest navigate … -> allow`, `guest loading …`, `the guest is up` — then
confirm the `dsh` window is visible and drawing dsh rather than dsh's 401 text.
Two stalls are not hand-off failures: the update question is answered before the
hand-off starts, and pointing `DSH_TAURI_REGISTRY` at an unreachable URL settles
it without writing to the do-not-remind store. A `dsh` window left hidden behind
the splash, by contrast, means the hand-off never completed and says nothing
about the cookie.

Whether a synthesised X11 key reaches a root-window grab could **not** be measured
under WSLg: `XTEST` events never arrived, with `global-hotkey` in isolation, which
makes that a limitation of the environment rather than evidence about the code.
The steps up to it are verified (the app runs as an X11 client, all five grabs
register, focus transitions fire).
