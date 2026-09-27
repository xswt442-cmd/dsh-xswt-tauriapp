//! The one way this crate writes back a setting it owns.
//!
//! Four memories live as small JSON documents: the port last asked for
//! ([`crate::ports`]), the zoom factor ([`crate::zoom`]), the guest window's shape
//! ([`crate::geometry`]), and the versions not to be reminded about
//! ([`crate::updates::dismiss`]). Each is read back tolerantly — a file that does
//! not parse reads as *nothing remembered*, which is the right answer for a
//! preference and the reason no reader here fails hard.
//!
//! That tolerance is exactly what makes the write worth doing carefully. A torn
//! file is not an error anyone sees: the next launch quietly comes up with the
//! default port, at 100% zoom, with the window somewhere else. So every one of
//! these writes through this module, and writes it as a rename — the bytes go to
//! a sibling `*.part` and only then move into place, which means a file a reader
//! can open is a file that was written whole.

use std::path::Path;

/// The tail on the name a setting is written under before it is in place.
const PART_SUFFIX: &str = ".part";

/// Write `value` to `path` as pretty JSON, arriving whole or not at all.
///
/// Parent directories are created, because every caller keeps its settings under
/// a directory of its own that may not exist on the first run.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
    }
    let text =
        serde_json::to_string_pretty(value).map_err(|error| format!("序列化失败：{error}"))?;
    let mut name = path
        .file_name()
        .ok_or_else(|| format!("{} 不是一个可写入的文件名", path.display()))?
        .to_os_string();
    name.push(PART_SUFFIX);
    let part = path.with_file_name(name);
    std::fs::write(&part, text).map_err(|error| format!("写入失败：{error}"))?;
    std::fs::rename(&part, path).map_err(|error| {
        let _ = std::fs::remove_file(&part);
        format!("写入失败：{error}")
    })
}

#[cfg(test)]
mod tests {
    use super::write_json;
    use serde::Serialize;
    use std::path::Path;

    #[derive(Serialize)]
    struct Setting<'a> {
        value: &'a str,
    }

    fn write(dir: &std::path::Path, name: &str, value: &str) -> Result<(), String> {
        write_json(&dir.join(name), &Setting { value })
    }

    #[test]
    fn a_setting_arrives_whole_or_not_at_all() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("setting.json");
        write(dir.path(), "setting.json", "first").expect("the first write");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "{\n  \"value\": \"first\"\n}"
        );

        // Overwriting leaves no intermediate state a reader could open: whatever
        // was there is either still there or fully replaced.
        write(dir.path(), "setting.json", "second").expect("the second write");
        assert!(std::fs::read_to_string(&path)
            .expect("read back")
            .contains("second"));
        assert!(
            !path.with_extension("json.part").exists(),
            "the staging name does not stay behind"
        );

        // A missing parent is created rather than being the caller's problem.
        let nested = dir.path().join("deeper").join("setting.json");
        write_json(&nested, &Setting { value: "third" }).expect("the nested write");
        assert!(nested.exists());
    }

    #[test]
    fn a_path_with_no_file_name_in_it_is_refused() {
        // Refused before anything is created, so a bad path cannot leave a
        // document half-written where a reader will find one.
        assert!(write_json(Path::new(""), &Setting { value: "x" }).is_err());
    }
}
