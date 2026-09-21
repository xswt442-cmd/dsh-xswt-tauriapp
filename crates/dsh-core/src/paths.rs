//! Where things are on this machine.
//!
//! The Harness home, the dsh launcher, and the `node` and `npm` that belong to
//! that install. Nothing here reaches the network or starts a process: this
//! module answers "which file", which is what the layers above need before they
//! can do either.

use std::fs;
use std::path::{Path, PathBuf};

/// The Harness home: `$DSH_HOME` when it points at an existing directory, else
/// `~/.dsh`.
pub fn dsh_home() -> PathBuf {
    if let Ok(raw) = std::env::var("DSH_HOME") {
        if !raw.is_empty() {
            let path = PathBuf::from(raw);
            if path.is_dir() {
                return path;
            }
        }
    }
    home_dir().join(".dsh")
}

/// The invoking user's home directory.
///
/// A normal Windows process does not get `HOME`, only `USERPROFILE`, so both
/// are consulted. Falling back to the filesystem root keeps the previous
/// behaviour when neither is set.
pub fn home_dir() -> PathBuf {
    let read = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    read("HOME")
        .or_else(|| read("USERPROFILE"))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Where the `server-<port>.{out,err}.log` files the token is read back from
/// live.
///
/// Not something dsh owns — it does not write this directory and does not know
/// the name. A launcher writing here is what puts a spawned server's stdout
/// somewhere the token can be recovered from, so the path and the file naming
/// are an external convention rather than this application's to change.
pub fn log_dir() -> PathBuf {
    dsh_home().join("launcher").join("logs")
}

/// File names a `node` executable may have, most likely first.
///
/// Windows PATH entries point at `node.exe`, so probing the bare name there
/// would never match.
pub const NODE_EXE_NAMES: &[&str] = if cfg!(windows) {
    &["node.exe", "node"]
} else {
    &["node"]
};

/// File names an `npm` launcher may have, most likely first.
///
/// npm installs `npm`, `npm.cmd` and `npm.ps1` side by side on every platform,
/// so the answer is the platform's and not the directory listing's: the
/// extensionless `npm` is a POSIX shell script, and on Windows it is the one
/// file of the three that `CreateProcess` cannot start at all — the
/// `os error 193` ("%1 不是有效的 Win32 应用程序") a bare first match produced.
pub const NPM_EXE_NAMES: &[&str] = if cfg!(windows) {
    &["npm.cmd", "npm.exe", "npm"]
} else {
    &["npm"]
};

/// Candidate `node` executables, most specific first.
pub fn node_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("DSH_NODE_BIN") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in NODE_EXE_NAMES {
                out.push(dir.join(name));
            }
        }
    }
    out
}

/// The node prefix that owns an installed dsh launcher.
///
/// A launcher sits at `<prefix>/node_modules/@deepseek-ai/dsh/lib/bin.js` — or
/// at `<prefix>/lib/node_modules/...`, which is how a Unix node prefix lays it
/// out (nvm, Homebrew, a distribution package). Walking up to the
/// `node_modules` boundary therefore names the prefix dsh is installed under,
/// and with it the `node` and `npm` that belong to that install.
///
/// The boundary alone is not enough to identify one. A Harness home keeps its
/// own module root at `$DSH_HOME/profiles/node_modules`, and a dsh resolved
/// through that would otherwise be credited to `<DSH_HOME>/profiles` — a
/// directory that owns neither node nor npm. So a candidate has to really hold
/// the npm that would run here, and callers canonicalise the launcher first, so
/// that the installation is named rather than the symlink farm in front of it.
pub fn node_prefix_of(launcher: &Path) -> Option<PathBuf> {
    layout_prefix(launcher, holds_npm)
}

/// The walk over the layouts, with the acceptance test injected.
///
/// Split from [`node_prefix_of`] so the shapes themselves are pinned without an
/// install on disk: a machine has one layout, and both have to be tested.
fn layout_prefix(launcher: &Path, accepts: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut dir = launcher.parent()?;
    loop {
        let parent = dir.parent()?;
        if dir.file_name().is_some_and(|name| name == "node_modules") {
            // The two layouts name their prefix one level apart: npm's Windows
            // layout has no `lib` layer between them.
            let candidate = if parent.file_name().is_some_and(|name| name == "lib") {
                parent.parent()
            } else {
                Some(parent)
            };
            if let Some(prefix) = candidate.filter(|candidate| accepts(candidate)) {
                return Some(prefix.to_path_buf());
            }
        }
        dir = parent;
    }
}

/// Whether `dir` is an npm prefix: it holds `npm` at its root, which is npm's
/// Windows layout, or under `bin`, which is a Unix node prefix.
fn holds_npm(dir: &Path) -> bool {
    npm_in(dir).is_some() || npm_in(&dir.join("bin")).is_some()
}

/// The `npm` launcher in `dir`, if it has one.
pub fn npm_in(dir: &Path) -> Option<PathBuf> {
    NPM_EXE_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The `node` that owns the dsh this process is going to run.
///
/// Preferred over `PATH`, and not merely for tidiness: a desktop launch inherits
/// a minimal `PATH` where the first `node` is often an older system one, and dsh
/// requires a much newer one. Taking that node produces a server that never
/// comes up, or an `npm install -g` into a prefix nobody uses.
///
/// `None` when the prefix holds no node. A custom `npm config prefix` is a bin
/// output directory and need not contain a node at all — on Windows it can be a
/// tree with nothing to do with where node itself is installed — and `PATH` is
/// what a caller falls back to there.
pub fn node_for_dsh() -> Option<PathBuf> {
    let launcher = fs::canonicalize(resolve_dsh_bin()?).ok()?;
    let prefix = node_prefix_of(&launcher)?;
    // A Unix node prefix keeps its executable in `bin`; npm's Windows layout
    // puts `node.exe` at the prefix root.
    [prefix.join("bin"), prefix]
        .into_iter()
        .find_map(|dir| node_in(&dir))
}

/// The `node` executable in `dir`, if it has one.
fn node_in(dir: &Path) -> Option<PathBuf> {
    NODE_EXE_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The first existing `node` candidate: the one that owns dsh, else `PATH`.
pub fn resolve_node() -> Option<PathBuf> {
    node_for_dsh().or_else(|| node_candidates().into_iter().find(|p| p.is_file()))
}

/// Candidate `dsh` launcher scripts (`lib/bin.js`), most specific first.
///
/// `dsh` on PATH is usually a symlink into a global `node_modules`, so the
/// usable entry point is always `.../@deepseek-ai/dsh/lib/bin.js`.
pub fn dsh_bin_candidates() -> Vec<PathBuf> {
    let suffix = Path::new("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    let mut out = Vec::new();
    if let Ok(explicit) = std::env::var("DSH_BIN") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }
    out.push(dsh_home().join("profiles").join(&suffix));
    if let Ok(prefix) = std::env::var("npm_config_prefix") {
        if !prefix.is_empty() {
            out.push(PathBuf::from(prefix).join(&suffix));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            // `.../bin/dsh` -> `.../bin/node_modules/...`, which is where npm
            // puts it for a bin-prefix install.
            out.push(dir.join(&suffix));
            // `.../bin/dsh` -> `.../lib/node_modules/...`, the nvm layout.
            if let Some(parent) = dir.parent() {
                out.push(parent.join("lib").join(&suffix));
            }
        }
    }
    out
}

/// The first existing `dsh` launcher.
pub fn resolve_dsh_bin() -> Option<PathBuf> {
    dsh_bin_candidates().into_iter().find(|p| p.is_file())
}

/// The version of the installed dsh, read from the resolved launcher's
/// `package.json` (`…/dsh/lib/bin.js` → `…/dsh/package.json`).
pub fn installed_version() -> Option<String> {
    let bin = resolve_dsh_bin()?;
    let manifest = bin.parent()?.parent()?.join("package.json");
    let text = fs::read_to_string(manifest).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{layout_prefix, node_prefix_of, NPM_EXE_NAMES};

    /// A launcher at the path dsh's own package layout puts it at.
    fn write_launcher(modules: &Path) -> std::path::PathBuf {
        let launcher = modules
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        std::fs::create_dir_all(launcher.parent().expect("parent")).expect("create");
        std::fs::write(&launcher, "").expect("write");
        launcher
    }

    #[test]
    fn a_prefix_is_named_with_or_without_the_lib_layer() {
        // The acceptance test is injected so both layouts are testable here: a
        // machine has one of them, and the walk has to know both.
        let any = |_: &Path| true;

        // A Unix node prefix puts `node_modules` under `lib`.
        assert_eq!(
            layout_prefix(
                Path::new("/opt/node/lib/node_modules/@deepseek-ai/dsh/lib/bin.js"),
                any
            ),
            Some(Path::new("/opt/node").to_path_buf())
        );
        assert_eq!(
            layout_prefix(
                Path::new(
                    "/home/u/.nvm/versions/node/v24.21.0/lib/node_modules/@deepseek-ai/dsh/lib/bin.js"
                ),
                any
            ),
            Some(Path::new("/home/u/.nvm/versions/node/v24.21.0").to_path_buf())
        );
        // npm's own layout has no `lib` layer, and the `lib` further in belongs
        // to the package — so the boundary is the first `node_modules`, not the
        // literal pair `lib/node_modules`. This is the shape that used to answer
        // `None` for every install made on Windows.
        assert_eq!(
            layout_prefix(
                Path::new("/nodejs/node_modules/@deepseek-ai/dsh/lib/bin.js"),
                any
            ),
            Some(Path::new("/nodejs").to_path_buf())
        );
        // Nothing to name without a `node_modules` boundary.
        assert_eq!(layout_prefix(Path::new("/usr/local/bin/dsh"), any), None);
    }

    #[test]
    fn a_module_root_that_owns_no_npm_is_not_a_prefix() {
        let dir = tempfile::tempdir().expect("temp dir");

        // `$DSH_HOME/profiles/node_modules` has the shape of an npm prefix and
        // owns no npm, so it must not be taken for one — the installation it is
        // a symlink farm into is the one to name.
        let profiles = dir.path().join("profiles");
        let launcher = write_launcher(&profiles.join("node_modules"));
        assert_eq!(node_prefix_of(&launcher), None);

        // The same tree under a directory that does own npm: now it is one.
        let global = dir.path().join("global");
        std::fs::create_dir_all(&global).expect("create");
        std::fs::write(global.join(NPM_EXE_NAMES[0]), "").expect("write");
        let launcher = write_launcher(&global.join("node_modules"));
        assert_eq!(node_prefix_of(&launcher), Some(global));
    }
}
