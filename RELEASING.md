# Releasing

Releases are tag-driven. Develop and verify on `dev`, then merge the release
commit into `main`. Only release-ready changes belong on `main`.

## Checklist

1. On `dev`, choose `X.Y.Z` and update:
   - `package.json#version`
   - `src-tauri/Cargo.toml#version`
   - `crates/dsh-core/Cargo.toml#version`
   - `src-tauri/tauri.conf.json#version`
   - `plugins/dsh-desktop-app/package.json#version` (the marketplace stub ships
     with the release it installs, so the tag contract covers it too)
   - the first section of both changelogs: `## X.Y.Z - YYYY-MM-DD`
   - the deb example in both READMEs: `dsh-xswt-tauriapp_X.Y.Z_amd64.deb`
   - both `Cargo.lock` files, which any cargo command rewrites to match — check
     `git status` before committing, because nothing else notices if they lag.
2. Run:

   ```sh
   cargo test   --manifest-path crates/dsh-core/Cargo.toml
   cargo test   --manifest-path src-tauri/Cargo.toml --lib
   node --test  plugins/dsh-desktop-app/test/plugin.test.js
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

The release workflow validates that the tag matches all five version fields,
rebuilds the deb and AppImage, packs the marketplace stub, and creates — or
refreshes — the GitHub release with those artifacts and the notes taken from
`CHANGELOG.md`. Tags are not moved; cut a new patch release instead.

## Changelog and release notes

The matching section of `CHANGELOG.md` *is* the release body, appended verbatim
to the fixed install block the workflow writes — so whatever goes in the
changelog is what a visitor to the release page reads. The workflow reads that
section from the **default branch**, not from the tag: at release time they are
the same commit, and afterwards the branch is the copy that keeps being edited,
so fixing an older entry is enough — re-running that tag restores the corrected
text rather than what the tag froze.

Entries are one line each and state the change in technical terms: what is now
true. How it was found, which file was at fault, and the measurements behind a
decision belong in the commit message and in `testplace/WORKLOG.md`, not here.
Both changelogs carry the same sections with the same number of entries in each;
`scripts/check-docs.mjs` fails otherwise.

## The marketplace asset

`stub` packs `plugins/dsh-desktop-app/` with `npm pack` and attaches it as
`dsh-xswt-tauriapp-plugin.tgz`. That asset is what the
[awesome-dsh-plugin](https://github.com/awesome-dsh-plugin/awesome-dsh-plugin)
entry points at, through a version-free name:

```
https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest/download/dsh-xswt-tauriapp-plugin.tgz
```

Two things follow from that URL. The name must stay version-free, because
`latest/download/` resolves only `latest` and takes the filename literally. And
the entry can only be submitted once some release already carries the asset, so
the stub ships in the release *before* the listing PR — a PR pointing at an asset
that does not exist yet sends reviewers to a 404.
