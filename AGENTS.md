# Agent guide

`DSH XSWTauri` is a Tauri shell for DeepSeek Harness: it reuses or starts a
local `dsh web` server, embeds it, and offers dsh updates on launch. It does not
modify, patch or vendor dsh.

## Workflow

- Develop on `dev`; keep `main` release-only.
- Lowercase Conventional Commit prefixes. Never use `--no-verify`.
- `testplace/` is ignored scratch — notes go there, not into tracked source.
- Read `RELEASING.md` only when preparing a release.

## Engineering

- Prefer root-cause fixes over workarounds.
- Non-GUI logic belongs in `crates/dsh-core`, which must keep building and
  testing without webkit2gtk.
- dsh is reached only over its local HTTP surface.
- Keep discovery as the Electron shell has it: ports 3080–3129, token from
  `$DSH_HOME/launcher/logs/server-<port>.out.log`, two-step handshake, detached
  server that is never stopped when the window closes.
- Never offer an update from a channel less stable than the installed one, and
  keep "don't remind me" scoped to one version.
- Keep the three version fields equal, and the README/CHANGELOG pairs in sync.

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

`cargo run --example launch --manifest-path crates/dsh-core/Cargo.toml` boots a
real server through the shell's own code path and prints its URL.
