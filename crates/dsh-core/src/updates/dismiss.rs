//! The versions the user asked not to be reminded about.
//!
//! One file under the application config directory, shaped like every other
//! small store here: a corrupt file reads as empty, and no failure to read or
//! write it can stop a startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Versions the user asked not to be reminded about, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DismissStore {
    /// Dismissed version strings.
    #[serde(default)]
    pub dismissed: Vec<String>,
    /// Where the list is persisted. Absent for in-memory use.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl DismissStore {
    /// An in-memory store, for tests and for a failed load.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, falling back to an empty list when the file is
    /// missing or unreadable. A corrupt file must never block startup.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut store = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        store.path = Some(path);
        store
    }

    /// Persist the list, creating parent directories as needed.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|error| format!("序列化失败：{error}"))?;
        std::fs::write(path, text).map_err(|error| format!("写入失败：{error}"))
    }

    /// Whether `version` is on the list.
    pub fn is_dismissed(&self, version: &str) -> bool {
        self.dismissed.iter().any(|entry| entry == version)
    }

    /// Add `version` and persist.
    pub fn dismiss(&mut self, version: &str) -> Result<(), String> {
        if !self.is_dismissed(version) {
            self.dismissed.push(version.to_string());
        }
        self.save()
    }

    /// Forget everything, so the next launch prompts again.
    pub fn clear(&mut self) -> Result<(), String> {
        self.dismissed.clear();
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dismiss_store_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("updates.json");

        let mut store = DismissStore::load(&path);
        assert!(store.dismissed.is_empty(), "missing file loads as empty");
        store.dismiss("0.1.6-alpha.1").unwrap();
        store.dismiss("0.1.6-alpha.1").unwrap();

        let reloaded = DismissStore::load(&path);
        assert_eq!(reloaded.dismissed, vec!["0.1.6-alpha.1".to_string()]);
        assert!(reloaded.is_dismissed("0.1.6-alpha.1"));
        assert!(!reloaded.is_dismissed("0.1.6-alpha.2"));

        let mut cleared = reloaded.clone();
        cleared.clear().unwrap();
        assert!(DismissStore::load(&path).dismissed.is_empty());
    }

    #[test]
    fn corrupt_dismiss_file_does_not_break_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(DismissStore::load(&path).dismissed.is_empty());
    }
}
