# Releasing

Releases are tag-driven. Develop and verify on `dev`, then merge the release
commit into `main`. Only release-ready changes belong on `main`.

## Checklist

1. On `dev`, choose `X.Y.Z` and update:
   - `package.json#version`
   - `src-tauri/Cargo.toml#version`
   - `crates/dsh-core/Cargo.toml#version`
   - `src-tauri/tauri.conf.json#version`
   - the first section of both changelogs: `## X.Y.Z - YYYY-MM-DD`
2. Run:

   ```sh
   cargo test   --manifest-path crates/dsh-core/Cargo.toml
   cargo test   --manifest-path src-tauri/Cargo.toml --lib
   cargo fmt    --manifest-path crates/dsh-core/Cargo.toml --check
   cargo fmt    --manifest-path src-tauri/Cargo.toml --check
   cargo clippy --manifest-path crates/dsh-core/Cargo.toml --all-targets -- -D warnings
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   node --input-type=module --check < web/main.js
   node scripts/check-docs.mjs
   pnpm tauri build
   ```

   There is no workspace manifest, so every cargo command names its crate.
   The bundle step is the only one that links the GUI; `cargo test` alone does
   not, so a broken webkit2gtk dependency would otherwise reach a tag. It bundles
   what `tauri.conf.json` declares rather than a platform's list, so it is valid
   anywhere — the first Windows run downloads the NSIS/WiX toolchain.
3. Commit and push the development branch, then merge it into `main` after CI
   passes.
4. From the release commit on `main`, create and push the `vX.Y.Z` tag:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

The release workflow validates that the tag matches all three version fields,
rebuilds the deb and AppImage, and creates — or refreshes — the GitHub release
with those artifacts and the notes taken from `CHANGELOG.md`. Tags are not
moved; cut a new patch release instead.
