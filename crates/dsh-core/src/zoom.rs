//! The guest window's zoom factor: its bounds, its startup value, and where it
//! is remembered.
//!
//! dsh's interface is dense, and the shell renders it at whatever scale the
//! webview is given. On a desktop whose applications are scaled — Windows at
//! 125%, 150% — a WSLg session is the awkward case: its compositor reports
//! `scale:1` however the Windows display is scaled, so every window there is
//! physically smaller than a native one, and the page with the most text is the
//! one that suffers.
//!
//! `set_zoom` is the shell's own lever and the only one that takes a fractional
//! factor, so the factor is worth keeping between launches: on WSLg the
//! environment cannot be fixed from inside, and re-applying it by hand on every
//! start is the whole of the complaint. It is also worth pinning from outside for
//! a session that is configured as a whole (say `~/.wslgconfig`), which is what
//! [`ZOOM_ENV`] is for.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Smallest factor the shell will apply.
pub const MIN_ZOOM: f64 = 0.3;
/// Largest factor the shell will apply.
pub const MAX_ZOOM: f64 = 3.0;
/// Environment variable that pins the factor for a session. Read once, at
/// startup, and it wins over what was remembered — the same precedence the
/// marketplace stub gives its own environment, and the same one the Wayland
/// opt-out uses.
pub const ZOOM_ENV: &str = "DSH_SHELL_ZOOM";

/// Clamp a factor into the usable range.
///
/// `None` for a value that is not a usable number at all, so a caller can tell
/// "out of range, bring it back" from "this is not a factor".
pub fn clamp(factor: f64) -> Option<f64> {
    if !factor.is_finite() || factor <= 0.0 {
        return None;
    }
    Some(factor.clamp(MIN_ZOOM, MAX_ZOOM))
}

/// The factor to start at: the environment wins, then what was remembered, then
/// 1.0. An unparsable environment value is ignored rather than fatal — a typo in
/// a session-wide config file must not leave the shell unable to start.
pub fn initial(stored: Option<f64>, from_env: Option<&str>) -> f64 {
    from_env
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .and_then(clamp)
        .or_else(|| stored.and_then(clamp))
        .unwrap_or(1.0)
}

/// The remembered zoom factor.
///
/// Deliberately shaped like [`crate::server::PortMemory`]: a tiny JSON file under
/// the application config directory, a corrupt file read as "nothing
/// remembered", and no error that can stop startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoomMemory {
    /// The factor the user last settled on.
    #[serde(default = "one")]
    pub factor: f64,
    /// Where the value is persisted. Absent for in-memory use.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// Serde default: a file that predates the field, or lost it, means 1.0.
fn one() -> f64 {
    1.0
}

impl Default for ZoomMemory {
    fn default() -> Self {
        Self {
            factor: 1.0,
            path: None,
        }
    }
}

impl ZoomMemory {
    /// An in-memory store, for tests and for a failed load.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, falling back to 1.0 when the file is missing or
    /// unreadable. A corrupt file must never block startup.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut memory = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        memory.path = Some(path);
        memory
    }

    /// The remembered factor, clamped to something usable.
    pub fn factor(&self) -> f64 {
        clamp(self.factor).unwrap_or(1.0)
    }

    /// Remember `factor` and persist it.
    pub fn remember(&mut self, factor: f64) -> Result<(), String> {
        self.factor = clamp(factor).unwrap_or(1.0);
        self.save()
    }

    /// Persist the value, creating parent directories as needed.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|error| format!("序列化失败：{error}"))?;
        fs::write(path, text).map_err(|error| format!("写入失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_factor_that_is_not_a_number_is_refused() {
        assert_eq!(clamp(0.0), None);
        assert_eq!(clamp(-1.5), None);
        assert_eq!(clamp(f64::NAN), None);
        assert_eq!(clamp(f64::INFINITY), None);
    }

    #[test]
    fn a_factor_out_of_range_is_brought_back() {
        assert_eq!(clamp(0.1), Some(MIN_ZOOM));
        assert_eq!(clamp(99.0), Some(MAX_ZOOM));
        assert_eq!(clamp(1.21), Some(1.21));
    }

    #[test]
    fn the_environment_wins_over_what_was_remembered() {
        assert_eq!(initial(Some(1.5), Some("1.25")), 1.25);
        // A value that cannot be parsed is ignored, not fatal.
        assert_eq!(initial(Some(1.5), Some("big")), 1.5);
        assert_eq!(initial(Some(1.5), Some("")), 1.5);
        // Nor is one that lands outside the range.
        assert_eq!(initial(Some(1.5), Some("100")), MAX_ZOOM);
        assert_eq!(initial(Some(1.5), Some("0")), 1.5);
        // Whitespace is the shape an env file usually has.
        assert_eq!(initial(None, Some(" 1.25 ")), 1.25);
    }

    #[test]
    fn nothing_remembered_and_nothing_pinned_is_one() {
        assert_eq!(initial(None, None), 1.0);
    }

    #[test]
    fn a_remembered_factor_survives_a_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("zoom.json");

        let mut memory = ZoomMemory::load(&path);
        assert_eq!(memory.factor(), 1.0);
        memory.remember(1.21).expect("remember");

        let reloaded = ZoomMemory::load(&path);
        assert_eq!(reloaded.factor(), 1.21);
    }

    #[test]
    fn a_corrupt_or_unusable_file_reads_as_one() {
        let dir = tempfile::tempdir().expect("temp dir");

        let corrupt = dir.path().join("corrupt.json");
        fs::write(&corrupt, "{ not json").expect("write");
        assert_eq!(ZoomMemory::load(&corrupt).factor(), 1.0);

        let silly = dir.path().join("silly.json");
        fs::write(&silly, r#"{"factor": 1000.0}"#).expect("write");
        assert_eq!(ZoomMemory::load(&silly).factor(), MAX_ZOOM);

        // An in-memory store has nowhere to write, and saying so is not a failure.
        assert!(ZoomMemory::in_memory().save().is_ok());
    }
}
