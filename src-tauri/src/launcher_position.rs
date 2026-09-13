//! Where the launcher shows, and where the person left it (D155).
//!
//! The search panel is the drag handle (`data-tauri-drag-region="deep"` in
//! `Launcher.svelte`). Once the person has dragged the window, every later
//! show — including after a restart — puts it back where they left it, as
//! long as the *handle* is still on a monitor.
//! Until they drag, nothing is written and the platform default applies
//! (macOS: next to the tray; Windows / X11: centred). Wayland places windows
//! itself, so there the position is neither saved nor restored.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use tauri::{PhysicalPosition, PhysicalRect};
use tauri_plugin_positioner::{Position, WindowExt};

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
/// `the_handle_offset_matches_the_stylesheet` in `Launcher.test.ts` reads both
/// this line and the stylesheet, and checks that same sum against the window
/// width, failing if any of them disagree.
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

#[derive(Default)]
struct Slots {
    /// What the last show applied — settled on the first focus-in after that
    /// show, not read synchronously: on GTK, `outer_position` right after
    /// `show()` is a cache fed only by the compositor's configure event, so a
    /// read that early is still the position from before this show (measured:
    /// the first Esc after a show on X11 wrote the platform default as a
    /// drag). `None` while a show is `awaiting` its settle.
    applied: Option<PhysicalPosition<i32>>,
    /// Where the person left the window: set only when a focus loss finds the
    /// window somewhere other than `applied`. Wins over the file for the rest
    /// of the session, so a failed write costs only the next restart — but a
    /// show that could not restore it (`placed` with `restored: None`) drops
    /// it again: the window is no longer there, so it is no longer evidence.
    left: Option<PhysicalPosition<i32>>,
    /// A show has happened and its focus-in settle has not arrived yet.
    awaiting: bool,
}

/// Managed state (`app.manage(Memory::default())`): one launcher per process.
#[derive(Default)]
pub struct Memory {
    slots: Mutex<Slots>,
}

impl Memory {
    fn lock(&self) -> MutexGuard<'_, Slots> {
        // Poisoning absorbed, as `PREFS_LOCK` does: two `Option`s cannot be
        // left half-written by a panic.
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn applied(&self) -> Option<PhysicalPosition<i32>> {
        self.lock().applied
    }
    pub fn left(&self) -> Option<PhysicalPosition<i32>> {
        self.lock().left
    }
    pub fn set_applied(&self, p: Option<PhysicalPosition<i32>>) {
        self.lock().applied = p;
    }
    /// The show has placed (or, when nothing was reachable, defaulted) the
    /// window. `applied` is unknown until the window's next focus-in settles
    /// it: on GTK the position read right after `show()` is a cache the
    /// window manager has not yet updated, and elsewhere a window manager may
    /// still move the window. When nothing was restored, the window is no
    /// longer where the person left it, so `left` goes; the file keeps the
    /// key — a monitor that comes back restores it.
    pub fn placed(&self, restored: Option<PhysicalPosition<i32>>) {
        let mut slots = self.lock();
        slots.applied = None;
        slots.awaiting = true;
        if restored.is_none() {
            slots.left = None;
        }
    }
    /// The launcher's focus-in after a show: what the show actually applied.
    /// Ignored when no show is awaiting one (a focus-in after an alt-tab).
    pub fn settled(&self, now: Option<PhysicalPosition<i32>>) {
        let mut slots = self.lock();
        if slots.awaiting {
            slots.applied = now;
            slots.awaiting = false;
        }
    }
    pub fn awaiting(&self) -> bool {
        self.lock().awaiting
    }
    /// The decision of D155: `now` is the person's choice only if it differs
    /// from the last position this process knows — where the person last
    /// left it, or, before any drag, where the application put it. With no
    /// show recorded at all there is nothing to compare against, and a
    /// position is not evidence of a drag. Records a move as `left` and
    /// returns it; `None` means "nothing to write".
    pub fn moved(&self, now: PhysicalPosition<i32>) -> Option<PhysicalPosition<i32>> {
        let mut slots = self.lock();
        let reference = slots.left.or(slots.applied)?;
        if reference == now {
            return None;
        }
        slots.left = Some(now);
        Some(now)
    }
}

/// `moved` + `write`, with the memory updated BEFORE the write so a write that
/// fails leaves the session behaving as dragged. Returns the write's result;
/// `Ok(())` also when there was nothing to write.
pub fn remember_position(
    now: PhysicalPosition<i32>,
    memory: &Memory,
    data_dir: &Path,
) -> std::io::Result<()> {
    match memory.moved(now) {
        Some(p) => write(data_dir, p),
        None => Ok(()),
    }
}

/// The one show path (D155): put the window where the person left it — from
/// memory first, then the file — if the handle is reachable; otherwise the
/// platform default. Then show and focus; what it actually applied is not
/// read back here — the launcher's next focus-in settles it (`Memory::settled`).
///
/// Order matters: `set_position` / `center` BEFORE `show` (no jump from the old
/// place); the macOS default `TrayCenter` AFTER `show`, as the positioner
/// wants it; `memory.placed` BEFORE `show`, marking the show as awaiting a
/// settle — reading `outer_position` here would be the GTK pre-map cache or,
/// on macOS/Windows, the point before `TrayCenter` ran.
///
/// Only for a HIDDEN launcher: `focus_launcher` sends a visible one to
/// `set_focus` alone, so a drag no focus loss has recorded yet survives a
/// single-instance callback (Linux: D-Bus, no focus change required).
///
/// `data_dir` is `None` where no `AppState` is managed (the shell tests), which
/// reads as "nothing saved". Wayland: no placement, no `applied` — the
/// compositor decides and the value GTK reports is not a position.
pub fn place<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    memory: &Memory,
    data_dir: Option<&Path>,
    wayland: bool,
) {
    let restored = if wayland {
        None
    } else {
        let saved = memory.left().or_else(|| data_dir.and_then(read));
        let monitors: Vec<(PhysicalRect<i32, u32>, f64)> = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| (*m.work_area(), m.scale_factor()))
            .collect();
        reachable(saved, &monitors)
    };
    match restored {
        Some(p) => {
            let _ = window.set_position(p);
        }
        // §6: next to the tray on macOS, placed after `show` below.
        None if wayland || cfg!(target_os = "macos") => {}
        None => {
            let _ = window.center();
        }
    }
    if !wayland {
        memory.placed(restored);
    }
    let _ = window.show();
    if restored.is_none() && !wayland && cfg!(target_os = "macos") {
        // No-ops where the tray position is unknown (mock runtime included).
        let _ = window.move_window(Position::TrayCenter);
    }
    let _ = window.set_focus();
}

/// Called from the `WindowEvent::Focused(false)` arm and the line before
/// `app.exit(0)`. Both call sites hide or exit a window that holds focus, so
/// all four dismissals (Esc, blur, the shortcut, a close) are *meant* to
/// arrive here as a focus loss (tao: `windowDidResignKey:` / `WM_KILLFOCUS`);
/// the live matrix walks each of the four separately, and a dismissal that
/// does not arrive would need its own call site instead. Nothing on Wayland
/// (the value GTK caches there is not a position — D153's sibling), nothing
/// when the runtime cannot say where the window is, and a failed write is
/// reported, not raised: this runs on the event loop.
pub fn remember<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    memory: &Memory,
    data_dir: &Path,
    wayland: bool,
) {
    if wayland {
        return;
    }
    let Ok(now) = window.outer_position() else {
        return;
    };
    if let Err(e) = remember_position(now, memory, data_dir) {
        eprintln!("launcher position not saved: {e}");
    }
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
            json!({"x": 4294967296i64, "y": 0}),
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

    // --- Memory: a hide without a drag writes nothing; a drag is written ---

    #[test]
    fn a_hide_without_a_drag_writes_nothing() {
        // The window is exactly where the application put it: that is not the
        // person's choice, so neither memory nor the file learns it — and the
        // next show still applies the platform default.
        let memory = Memory::default();
        memory.set_applied(at(300, 200));
        assert_eq!(memory.moved(PhysicalPosition::new(300, 200)), None);
        assert_eq!(memory.left(), None);
    }

    #[test]
    fn a_drag_is_written_and_kept_in_memory() {
        let memory = Memory::default();
        memory.set_applied(at(300, 200));
        assert_eq!(memory.moved(PhysicalPosition::new(640, 80)), at(640, 80));
        assert_eq!(memory.left(), at(640, 80));
        // A second focus loss at the same dragged place is not a second move:
        // the reference is now `left`, not `applied`.
        assert_eq!(memory.moved(PhysicalPosition::new(640, 80)), None);
        assert_eq!(memory.applied(), at(300, 200));
    }

    #[test]
    fn dragging_back_to_where_the_app_put_it_is_still_a_move() {
        // A → B → A (review P2-2): the person's last choice is A, and a rule
        // that compares with `applied` alone would keep B.
        let memory = Memory::default();
        memory.set_applied(at(300, 200));
        assert_eq!(memory.moved(PhysicalPosition::new(640, 80)), at(640, 80));
        assert_eq!(memory.moved(PhysicalPosition::new(300, 200)), at(300, 200));
        assert_eq!(memory.left(), at(300, 200));
    }

    #[test]
    fn a_show_that_could_not_restore_forgets_where_the_person_left_it() {
        // A drag is recorded, then a show cannot restore it (undock, DPI
        // change, whatever `reachable` returned None for) and falls back to
        // the platform default. The next untouched hide must not treat that
        // default as a new drag — `left` is stale and must go with it.
        let memory = Memory::default();
        memory.set_applied(at(300, 200));
        assert_eq!(memory.moved(PhysicalPosition::new(640, 80)), at(640, 80));
        memory.placed(None);
        memory.settled(at(460, 220));
        assert_eq!(memory.left(), None, "a failed restore kept the stale drag");
        assert_eq!(memory.applied(), at(460, 220));
        assert_eq!(memory.moved(PhysicalPosition::new(460, 220)), None);

        // Positive control: a show that DID restore the drag keeps `left`.
        let memory = Memory::default();
        memory.set_applied(at(300, 200));
        assert_eq!(memory.moved(PhysicalPosition::new(640, 80)), at(640, 80));
        memory.placed(at(640, 80));
        memory.settled(at(640, 80));
        assert_eq!(memory.left(), at(640, 80));
    }

    #[test]
    fn a_show_leaves_applied_open_until_the_focus_in_settles_it() {
        let memory = Memory::default();
        memory.placed(at(640, 80));
        assert_eq!(memory.applied(), None, "applied before any focus-in");
        assert!(memory.awaiting());
        memory.settled(at(640, 80));
        assert_eq!(memory.applied(), at(640, 80));
        assert!(!memory.awaiting());
        // A later focus-in (alt-tab back) settles nothing: applied stays.
        memory.settled(at(1, 1));
        assert_eq!(memory.applied(), at(640, 80));
    }

    #[test]
    fn a_blur_before_the_focus_in_writes_nothing() {
        // The stale-read case on GTK, made safe: nothing to compare against.
        let memory = Memory::default();
        memory.placed(None);
        assert_eq!(memory.moved(PhysicalPosition::new(280, 160)), None);
        assert_eq!(memory.left(), None);
    }

    #[test]
    fn nothing_applied_means_nothing_moved() {
        // No show recorded — the application never placed the window, so a
        // position is not evidence of a drag (review P2-1: start in the tray,
        // Quit; Wayland never records one either).
        let memory = Memory::default();
        assert_eq!(memory.moved(PhysicalPosition::new(1, 2)), None);
        assert_eq!(memory.left(), None);
    }

    #[test]
    fn a_quit_before_the_first_show_keeps_the_saved_position() {
        // prefs hold a position from an earlier session; the app starts into
        // the tray and quits without ever showing the launcher. The key must
        // be exactly what it was.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), PhysicalPosition::new(640, 80)).unwrap();
        let memory = Memory::default();
        remember_position(PhysicalPosition::new(0, 0), &memory, dir.path()).unwrap();
        assert_eq!(read(dir.path()), at(640, 80));
        // Positive control: the same call after a show at (0, 0) — still not
        // a move; after a show somewhere else — it is.
        memory.set_applied(at(0, 0));
        remember_position(PhysicalPosition::new(0, 0), &memory, dir.path()).unwrap();
        assert_eq!(read(dir.path()), at(640, 80));
        memory.set_applied(at(5, 5));
        remember_position(PhysicalPosition::new(0, 0), &memory, dir.path()).unwrap();
        assert_eq!(read(dir.path()), at(0, 0));
    }

    #[test]
    fn a_failed_write_still_updates_memory() {
        // `data_dir` is a FILE, so `create_dir_all` inside `write_key` fails on
        // every platform. The session must still behave as dragged.
        let dir = tempfile::tempdir().unwrap();
        let not_a_dir = dir.path().join("prefs-here-is-a-file");
        std::fs::write(&not_a_dir, b"").unwrap();
        let memory = Memory::default();
        memory.set_applied(at(0, 0));
        let outcome = remember_position(PhysicalPosition::new(50, 60), &memory, &not_a_dir);
        assert!(outcome.is_err(), "the write into a file path succeeded?");
        assert_eq!(memory.left(), at(50, 60));
    }

    #[test]
    fn a_successful_write_lands_the_dragged_position() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::default();
        memory.set_applied(at(0, 0));
        remember_position(PhysicalPosition::new(50, 60), &memory, dir.path()).unwrap();
        assert_eq!(read(dir.path()), at(50, 60));
    }
}
