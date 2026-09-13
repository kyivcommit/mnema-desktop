//! Where the launcher shows, and where the person left it (D155).
//!
//! The search panel is to become the drag handle (`data-tauri-drag-region="deep"`
//! in `Launcher.svelte`, a later commit of this branch). Once the person has
//! dragged the window, every later show — including after a restart — puts it
//! back where they left it, as long as the *handle* is still on a monitor.
//! Until they drag, nothing is written and the platform default applies
//! (macOS: next to the tray; Windows / X11: centred). Wayland places windows
//! itself, so there the position is neither saved nor restored.

use std::path::Path;

use tauri::{PhysicalPosition, PhysicalRect};

/// The `prefs.json` key: `{"x": <i32>, "y": <i32>}`, physical pixels of
/// `outer_position`.
pub const KEY: &str = "launcher_position";

/// A point on the drag handle, in logical pixels from the window's top-left
/// corner: the middle of the search panel's first row (the input and the pin).
/// Every number is a declaration in `ui/src/styles/launcher.css` — `main.panels`
/// padding `24px 32px`, first column 190, gap 16, second column 470, then the
/// panel's own padding-top 11 and the pin's height 26.
///
/// The offset is only this fixed because the window is: `"width": 1000` and
/// `"resizable": false` in `src-tauri/tauri.conf.json`. The three grid tracks
/// plus their two 16px gaps (190 + 470 + 244 + 2 × 16 = 936) exactly fill the
/// content box (1000 − 2 × 32 = 936), which is why `main.panels`'
/// `justify-content: center` and the middle column's `minmax(0, …)` floor never
/// actually engage — either one becoming live would shift the panel and make
/// this offset wrong.
/// `the_handle_offset_matches_the_stylesheet` in `Launcher.test.ts` is to read
/// both this line and the stylesheet, and check that same sum against the
/// window width, failing if any of them disagree (a later commit of this
/// branch).
pub const HANDLE_CENTRE: (f64, f64) = (32.0 + 190.0 + 16.0 + 470.0 / 2.0, 24.0 + 11.0 + 26.0 / 2.0);

/// The saved position, or `None` for anything that is not two integers under
/// `KEY`. Tolerant on purpose: this runs on show, with nowhere to report to,
/// exactly as `theme::read_choice`.
pub fn read(data_dir: &Path) -> Option<PhysicalPosition<i32>> {
    let all = crate::prefs::read_all(data_dir);
    let value = all.get(KEY)?;
    // `as_i64` is `None` for 1.5 and for "12"; `try_from` for anything past i32.
    let x = i32::try_from(value.get("x")?.as_i64()?).ok()?;
    let y = i32::try_from(value.get("y")?.as_i64()?).ok()?;
    Some(PhysicalPosition::new(x, y))
}

/// Writes the position, keeping every other key in the file.
pub fn write(data_dir: &Path, p: PhysicalPosition<i32>) -> std::io::Result<()> {
    crate::prefs::write_key(data_dir, KEY, serde_json::json!({ "x": p.x, "y": p.y }))
}

/// `Some(saved)` when the drag handle's reference point would land inside some
/// monitor's work area — `(x + HANDLE_CENTRE.0 * scale, y + HANDLE_CENTRE.1 * scale)`
/// with *that* monitor's scale factor, since the saved point is physical and
/// the handle offset is logical. Each monitor is tried with its own scale: a
/// window straddling a 1× and a 2× monitor takes the scale of whichever holds
/// most of it, and the monitor that holds the handle is the one whose scale
/// put it there — the approximation errs towards keeping a saved position, and
/// `adjacent_monitors_with_different_scales_each_use_their_own` pins it. Otherwise `None`: a monitor that was unplugged,
/// or a resolution / DPI change that left only the window's corner on screen.
/// The whole window need not be visible — a window the person left half off
/// the edge comes back half off the edge.
pub fn reachable(
    saved: Option<PhysicalPosition<i32>>,
    monitors: &[(PhysicalRect<i32, u32>, f64)],
) -> Option<PhysicalPosition<i32>> {
    let p = saved?;
    let on_some_monitor = monitors.iter().any(|(area, scale)| {
        let hx = f64::from(p.x) + HANDLE_CENTRE.0 * scale;
        let hy = f64::from(p.y) + HANDLE_CENTRE.1 * scale;
        let x0 = f64::from(area.position.x);
        let y0 = f64::from(area.position.y);
        hx >= x0
            && hx < x0 + f64::from(area.size.width)
            && hy >= y0
            && hy < y0 + f64::from(area.size.height)
    });
    on_some_monitor.then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tauri::PhysicalSize;

    fn monitor(x: i32, y: i32, w: u32, h: u32, scale: f64) -> (PhysicalRect<i32, u32>, f64) {
        (
            PhysicalRect {
                position: PhysicalPosition::new(x, y),
                size: PhysicalSize::new(w, h),
            },
            scale,
        )
    }
    fn at(x: i32, y: i32) -> Option<PhysicalPosition<i32>> {
        Some(PhysicalPosition::new(x, y))
    }

    // --- reachable: the two counterexamples from the spec review, then DPI ---

    #[test]
    fn a_handle_whose_centre_is_on_a_monitor_is_kept() {
        // Window corner off the left edge, handle (x = 373) well inside.
        let m = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(at(-100, 100), &m), at(-100, 100));
    }

    #[test]
    fn a_window_whose_corner_is_on_screen_but_whose_handle_is_not_is_dropped() {
        // Corner at x = 1800 is inside a 1920-wide monitor; the handle centre
        // (1800 + 473 = 2273) is not. The old corner test kept this one.
        let m = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(at(1800, 100), &m), None);
    }

    #[test]
    fn the_handle_offset_scales_with_the_monitor() {
        // Same physical point, same 3840-wide monitor: at scale 2 the handle
        // centre is 3000 + 946 = 3946 (off), at scale 1 it is 3473 (on).
        let hidpi = [monitor(0, 0, 3840, 2160, 2.0)];
        let lodpi = [monitor(0, 0, 3840, 2160, 1.0)];
        assert_eq!(reachable(at(3000, 200), &hidpi), None);
        assert_eq!(reachable(at(3000, 200), &lodpi), at(3000, 200));
    }

    #[test]
    fn the_work_area_edge_is_outside() {
        // Centre exactly at x0 + w is the first pixel that is NOT on the
        // monitor; one to the left is the last that is. Kills `<` → `<=`.
        let m = [monitor(0, 0, 1000, 1000, 1.0)];
        let on_edge = 1000 - 473;
        assert_eq!(reachable(at(on_edge, 0), &m), None);
        assert_eq!(reachable(at(on_edge - 1, 0), &m), at(on_edge - 1, 0));
    }

    #[test]
    fn the_work_area_bottom_edge_is_outside() {
        // The vertical twin: handle y = saved.y + 48. Kills a `y` offset that
        // drifts (48 → 148 leaves every horizontal test green) and `<` → `<=`.
        let m = [monitor(0, 0, 1000, 1000, 1.0)];
        let on_edge = 1000 - 48;
        assert_eq!(reachable(at(0, on_edge), &m), None);
        assert_eq!(reachable(at(0, on_edge - 1), &m), at(0, on_edge - 1));
    }

    #[test]
    fn a_window_above_the_top_edge_with_its_handle_below_it_is_kept() {
        // Corner 30 px above the monitor, handle (−30 + 48 = 18) inside. A
        // `reachable` that checks the corner's y — or ignores the y offset —
        // drops it.
        let m = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(at(100, -30), &m), at(100, -30));
        assert_eq!(reachable(at(100, -49), &m), None);
    }

    #[test]
    fn adjacent_monitors_with_different_scales_each_use_their_own() {
        // A 1× monitor at 0..1920 and a 2× monitor to its right. Saved corner at
        // x = 1600: on the 1× monitor the handle (2073) is off its edge, on the
        // 2× monitor (1600 + 946 = 2546) it is inside — kept. Remove the 2×
        // monitor and it is off everything.
        let two = [
            monitor(0, 0, 1920, 1080, 1.0),
            monitor(1920, 0, 3840, 2160, 2.0),
        ];
        let one = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(at(1600, 100), &two), at(1600, 100));
        assert_eq!(reachable(at(1600, 100), &one), None);
    }

    #[test]
    fn nothing_saved_places_nothing() {
        let m = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(None, &m), None);
    }

    #[test]
    fn a_second_monitor_with_negative_origin_counts() {
        // Primary at 0, a second monitor to its left. The point is off the
        // primary and on the second — and with the second removed it is off.
        let two = [
            monitor(0, 0, 1920, 1080, 1.0),
            monitor(-1920, 0, 1920, 1080, 1.0),
        ];
        let one = [monitor(0, 0, 1920, 1080, 1.0)];
        assert_eq!(reachable(at(-1500, 100), &two), at(-1500, 100));
        assert_eq!(reachable(at(-1500, 100), &one), None);
    }

    // --- the key ---

    #[test]
    fn the_position_survives_a_write_and_a_read() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), PhysicalPosition::new(-120, 45)).unwrap();
        assert_eq!(read(dir.path()), at(-120, 45));
        // And the raw shape is the documented one, not whatever serde chose.
        assert_eq!(
            crate::prefs::read_all(dir.path()).get(KEY),
            Some(&json!({"x": -120, "y": 45}))
        );
    }

    #[test]
    fn a_value_that_is_not_two_integers_reads_as_nothing() {
        for bad in [
            json!("12,34"),
            json!({"x": 12}),
            json!({"x": 1.5, "y": 2}),
            json!([12, 34]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            crate::prefs::write_key(dir.path(), KEY, bad.clone()).unwrap();
            assert_eq!(read(dir.path()), None, "accepted {bad}");
        }
        // Positive control: the same path with a good value reads back.
        let dir = tempfile::tempdir().unwrap();
        crate::prefs::write_key(dir.path(), KEY, json!({"x": 12, "y": 34})).unwrap();
        assert_eq!(read(dir.path()), at(12, 34));
    }
}
