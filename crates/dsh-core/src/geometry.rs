//! Where the dsh window was, so the next launch puts it back.
//!
//! Remembered for the same reason the zoom factor is: the dsh window is the daily
//! surface, and a shape chosen once — wide on a big display, or small beside an
//! editor — should not have to be chosen again at every launch.
//!
//! Two things can go wrong with a remembered geometry, and both are dealt with
//! here rather than at the window:
//!
//! * a file that describes something no window could be is a corrupt file, and
//!   restoring it would mean a window nobody can use;
//! * a position is only meaningful together with the displays that existed when
//!   it was recorded. Unplug the monitor the window was on and those coordinates
//!   are off-screen — the shell starts, the window is "shown", and there is
//!   nothing to click. The monitor list is the platform's, so it is passed in
//!   rather than looked up here ([`lands_on_a_display`]).
//!
//! # Units
//!
//! Physical pixels, deliberately. That is what the window API *reports*
//! ([`outer_position`](https://docs.rs/tauri) and `inner_size`) and what it
//! accepts back (`set_position`/`set_size`), so a geometry survives a display
//! whose scale factor is not the one it was recorded on — the case that makes
//! storing logical pixels wrong on Windows, where the scale can be 125% and every
//! round trip through it drifts. The one place logical pixels appear is the
//! window's own minimum size, which the platform keeps applying for us.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The smallest window this will restore, in physical pixels.
///
/// Deliberately far below the window's own minimum size (900×600 *logical*): what
/// is stored is physical, and 900 logical pixels is 1800 physical on a 200%
/// display, so a floor in physical pixels cannot be the logical minimum. This one
/// only has to recognise a file that is not a window; the real minimum keeps
/// being enforced by `min_inner_size` wherever the window is built.
pub const MIN_WIDTH: u32 = 300;
/// See [`MIN_WIDTH`].
pub const MIN_HEIGHT: u32 = 200;
/// A ceiling on what will be restored, for the same reason: no display is this
/// large, so a file that claims it is not a geometry.
pub const MAX_SIDE: u32 = 16_384;
/// How much of the window's top strip has to land on a display for its position
/// to be worth restoring: wide enough to grab with a pointer, and as tall as the
/// strip itself.
///
/// The top strip rather than any part of the window, because that is what a
/// window is moved by: a window whose bottom edge alone is on screen has its only
/// handle off screen, which is the state this check exists to prevent.
const GRABBABLE: (i64, i64) = (200, 40);

/// A window rectangle, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    /// Left edge of the frame, in virtual-screen coordinates.
    pub x: i32,
    /// Top edge of the frame.
    pub y: i32,
    /// Client width.
    pub width: u32,
    /// Client height.
    pub height: u32,
}

impl Geometry {
    /// Whether this describes a window that could exist at all.
    pub fn is_sane(&self) -> bool {
        (MIN_WIDTH..=MAX_SIDE).contains(&self.width)
            && (MIN_HEIGHT..=MAX_SIDE).contains(&self.height)
    }
}

/// A display's rectangle, as the window API reports it: physical pixels, which
/// is the same unit [`Geometry`] is kept in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Whether restoring `geometry` would put the window somewhere reachable.
///
/// `displays` is what is plugged in *now*; a remembered position on a display
/// that has since been unplugged is off-screen, and coming up there is
/// indistinguishable from the application not having started.
pub fn lands_on_a_display(geometry: &Geometry, displays: &[Display]) -> bool {
    // i64 throughout: the coordinates come from a file, and a saved `i32::MIN`
    // must not overflow into a plausible-looking overlap.
    let top_strip = i64::from(geometry.height).min(GRABBABLE.1);
    displays.iter().any(|display| {
        let across = overlap(
            i64::from(geometry.x),
            i64::from(geometry.width),
            i64::from(display.x),
            i64::from(display.width),
        );
        let down = overlap(
            i64::from(geometry.y),
            top_strip,
            i64::from(display.y),
            i64::from(display.height),
        );
        across >= GRABBABLE.0 && down >= top_strip
    })
}

/// How much of `[a_start, a_start + a_len)` lies inside `[b_start, b_start + b_len)`.
fn overlap(a_start: i64, a_len: i64, b_start: i64, b_len: i64) -> i64 {
    (a_start + a_len).min(b_start + b_len) - a_start.max(b_start)
}

/// The remembered geometry, and the file it is kept in.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowMemory {
    /// The geometry last seen, when it was one worth keeping.
    #[serde(default)]
    pub geometry: Option<Geometry>,
    /// Where the value is persisted. Absent for in-memory use.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl WindowMemory {
    /// An in-memory store, for tests and for a failed load.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, falling back to "no geometry" when the file is missing,
    /// unreadable or not a window. A corrupt file must never block startup, and
    /// must never be obeyed either.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut memory = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        if memory.geometry.is_some_and(|geometry| !geometry.is_sane()) {
            memory.geometry = None;
        }
        memory.path = Some(path);
        memory
    }

    /// The remembered geometry, when there is one worth using.
    pub fn geometry(&self) -> Option<Geometry> {
        self.geometry.filter(Geometry::is_sane)
    }

    /// Remember `geometry` and persist it.
    ///
    /// A geometry no window could have is ignored rather than written: the file
    /// is read back on the next launch, and there is no point in teaching it
    /// something it would then refuse.
    pub fn remember(&mut self, geometry: Geometry) -> Result<(), String> {
        if !geometry.is_sane() {
            return Ok(());
        }
        self.geometry = Some(geometry);
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

    fn display(x: i32, y: i32, width: u32, height: u32) -> Display {
        Display {
            x,
            y,
            width,
            height,
        }
    }

    fn at(x: i32, y: i32) -> Geometry {
        Geometry {
            x,
            y,
            width: 1500,
            height: 940,
        }
    }

    #[test]
    fn a_file_that_is_not_a_window_is_not_restored() {
        assert!(at(0, 0).is_sane());
        // Too small to be anything, and too large for any display.
        assert!(!Geometry {
            width: MIN_WIDTH - 1,
            ..at(0, 0)
        }
        .is_sane());
        assert!(!Geometry {
            height: MIN_HEIGHT - 1,
            ..at(0, 0)
        }
        .is_sane());
        assert!(!Geometry {
            width: MAX_SIDE + 1,
            ..at(0, 0)
        }
        .is_sane());
    }

    #[test]
    fn a_position_on_a_display_that_is_gone_is_not_restored() {
        let laptop = display(0, 0, 1920, 1080);
        // The ordinary case: the window was on the display that is still there.
        assert!(lands_on_a_display(&at(100, 100), &[laptop]));
        // A second display to the left, whose coordinates are negative — that is
        // a real position, not a corrupt one.
        let left = display(-1920, 0, 1920, 1080);
        assert!(lands_on_a_display(&at(-1800, 50), &[laptop, left]));
        // Unplug it and the same position is nowhere.
        assert!(!lands_on_a_display(&at(-1800, 50), &[laptop]));

        // The failure this check exists for: the window's bottom edge alone is on
        // screen, so its title bar — the only thing that moves it — is not.
        let mostly_above = Geometry {
            x: 0,
            y: -900,
            width: 1500,
            height: 940,
        };
        assert!(!lands_on_a_display(&mostly_above, &[laptop]));
        // Only a sliver across is not enough to grab either.
        let mostly_left = Geometry {
            x: -1480,
            y: 0,
            width: 1500,
            height: 940,
        };
        assert!(!lands_on_a_display(&mostly_left, &[laptop]));

        // With no displays at all there is nowhere to put it.
        assert!(!lands_on_a_display(&at(0, 0), &[]));
    }

    #[test]
    fn coordinates_from_a_file_cannot_overflow_into_a_plausible_position() {
        let absurd = Geometry {
            x: i32::MIN,
            y: i32::MIN,
            width: MAX_SIDE,
            height: MAX_SIDE,
        };
        assert!(absurd.is_sane());
        assert!(!lands_on_a_display(&absurd, &[display(0, 0, 1920, 1080)]));
    }

    #[test]
    fn a_geometry_survives_a_round_trip_and_a_corrupt_file_does_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("window.json");
        let mut memory = WindowMemory::load(&path);
        assert_eq!(memory.geometry(), None, "nothing has been written yet");

        memory.remember(at(120, 64)).expect("write");
        assert_eq!(WindowMemory::load(&path).geometry(), Some(at(120, 64)));

        // A file that parses but says something no window could be is dropped
        // rather than obeyed.
        fs::write(&path, r#"{"geometry":{"x":0,"y":0,"width":2,"height":2}}"#).expect("write");
        assert_eq!(WindowMemory::load(&path).geometry(), None);
        // And one that does not parse at all is simply no geometry.
        fs::write(&path, "not json").expect("write");
        assert_eq!(WindowMemory::load(&path).geometry(), None);
    }

    #[test]
    fn a_geometry_that_is_not_a_window_is_not_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("window.json");
        let mut memory = WindowMemory::load(&path);
        memory
            .remember(Geometry {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            })
            .expect("ignored");
        assert_eq!(memory.geometry(), None);
        assert!(!path.exists(), "nothing worth writing was written");
    }

    #[test]
    fn an_in_memory_store_keeps_the_value_and_grows_no_file() {
        let mut memory = WindowMemory::in_memory();
        memory.remember(at(10, 20)).expect("remembered");
        assert_eq!(memory.geometry(), Some(at(10, 20)));
        assert_eq!(memory.path, None);
    }
}
