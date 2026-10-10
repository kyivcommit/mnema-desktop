//! The launcher's width and where its search column sits in it. The numbers are
//! declarations in `ui/src/styles/launcher.css` (`main.panels`); the guard
//! `the handle offset matches the stylesheet` (`Launcher.test.ts`) reads both.
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Layout {
    pub left: bool,
    pub right: bool,
}

pub const SEARCH_WIDTH: f64 = 470.0;
pub const HEIGHT: f64 = 592.0;
pub const LEFT_SPAN: f64 = 285.0 + 5.0;
pub const RIGHT_SPAN: f64 = 366.0 + 5.0;

pub fn width(l: Layout) -> f64 {
    SEARCH_WIDTH + if l.left { LEFT_SPAN } else { 0.0 } + if l.right { RIGHT_SPAN } else { 0.0 }
}

/// How far the search column's left edge sits from the window's. Exact only
/// because `launcher.css` starts the tracks at the window's left edge and no
/// track is allowed to shrink; if either changed, the column would sit
/// elsewhere and this offset would be wrong.
pub fn search_offset(l: Layout) -> f64 {
    if l.left { LEFT_SPAN } else { 0.0 }
}

/// The layout the launcher window has now. Managed; read by `here`, `place`
/// and the cold show.
#[derive(Default)]
pub struct Current(Mutex<Layout>);

impl Current {
    pub fn get(&self) -> Layout {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn set(&self, l: Layout) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = l;
    }
}

/// When the launcher last hid or lost focus (a pinned one loses focus and
/// stays up; every hide that follows marks again); `None` until the first.
/// Managed; written by `hide_launcher` and the `Focused(false)` arm, read by
/// the show. A wall-clock time: `Instant` does not advance while the machine
/// sleeps, and sleep is time hidden.
#[derive(Default)]
pub struct HiddenAt(Mutex<Option<SystemTime>>);

impl HiddenAt {
    pub fn mark(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(SystemTime::now());
    }
    pub fn get(&self) -> Option<SystemTime> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Whether a show at `now` finds the launcher cold: hidden for at least
/// `minutes`. Never hidden (`None`) is not cold: the start-up layout already is.
/// A clock set back since the hide (`duration_since` fails) counts as elapsed,
/// so the person gets a fresh launcher and never a stale answer.
pub fn goes_cold(hidden_at: Option<SystemTime>, now: SystemTime, minutes: u32) -> bool {
    hidden_at.is_some_and(|t| {
        now.duration_since(t)
            .map_or(true, |d| d >= Duration::from_secs(u64::from(minutes) * 60))
    })
}

/// Sets the window to the size of layout `l`. The one place that turns a
/// layout into a window size, for `set_launcher_layout` and the cold show.
pub fn resize<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, l: Layout) {
    let _ = window.set_size(tauri::LogicalSize::new(width(l), HEIGHT));
}

/// The window frame a layout change asks for: the corner `refit` returns
/// and the size `width`/`HEIGHT` give, top-left origin as everywhere else in
/// this module. The macOS branch hands all of it to AppKit in one call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: crate::launcher_position::Point,
    pub width: f64,
    pub height: f64,
}

pub fn target_frame(origin: crate::launcher_position::Point, next: Layout) -> Frame {
    Frame {
        origin,
        width: width(next),
        height: HEIGHT,
    }
}

/// AppKit's y for a frame whose top edge is `top` below the top of the
/// primary screen: AppKit measures up from the primary screen's bottom edge.
pub fn appkit_origin_y(top: f64, height: f64, primary_height: f64) -> f64 {
    primary_height - top - height
}

/// Applies `frame` in ONE `setFrame:display:` call, so AppKit never draws the
/// in-between window (wider, still at the old x) that `set_size` followed by
/// `set_position` produces. Returns false if the call could not be made
/// (off the main thread, no handle), and the caller falls back to two steps.
///
/// The y flip uses the PRIMARY screen's height (`NSScreen.screens[0]`), not
/// the window's own screen: AppKit's global space has its origin at the
/// primary screen's bottom-left whichever monitor the window is on, and that
/// is also what tao's `outer_position` flips by, so the corner read there and
/// the frame written here agree on every monitor.
#[cfg(target_os = "macos")]
fn set_frame_once<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, frame: Frame) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    if ptr.is_null() {
        return false;
    }
    let Some(primary) = NSScreen::screens(mtm).firstObject() else {
        return false;
    };
    // SAFETY: tauri hands out the window's live `NSWindow*`; it is used only
    // here, on the main thread, within this command.
    let ns_window: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
    let y = appkit_origin_y(
        frame.origin.y.into(),
        frame.height,
        primary.frame().size.height,
    );
    let rect = NSRect::new(
        NSPoint::new(f64::from(frame.origin.x), y),
        NSSize::new(frame.width, frame.height),
    );
    ns_window.setFrame_display(rect, true);
    true
}

/// Resizes the launcher to its visible panels; the search column keeps its
/// place on screen (owner, 2026-09-25) unless the grown window would leave the
/// monitor's work area: then the window moves horizontally just enough to fit,
/// and the search column returns to where the person put it when the window
/// shrinks again (`launcher_position::refit`, `Memory::nudge`). Synchronous:
/// AppKit wants the window size from the main thread, as `open_settings`.
#[tauri::command]
pub fn set_launcher_layout<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    current: tauri::State<'_, Current>,
    memory: tauri::State<'_, crate::launcher_position::Memory>,
    left: bool,
    right: bool,
) {
    use crate::launcher_position::{Space, effective_nudge, monitors, refit, window_corner};
    use tauri::Manager;
    let next = Layout { left, right };
    let before = current.get();
    if before == next {
        return;
    }
    if let Some(window) = app.get_webview_window("launcher") {
        // Read in the old layout, put the window round it in the new one.
        let corner = if crate::os_services::wayland_session() {
            None
        } else {
            window_corner(&window.as_ref().window())
        };
        let placed = corner.map(|(c, factor)| {
            let nudge = effective_nudge(c, memory.placed_at(), memory.nudge());
            refit(c, before, next, factor, nudge, &monitors(&window))
        });
        // macOS: origin and size in one AppKit call (see `set_frame_once`).
        // ponytail: Windows and Linux keep two steps, `set_size` then
        // `set_position`; no flicker was reported there. If one shows up, give
        // them a single native call too (SetWindowPos / gtk_window_move_resize).
        #[cfg(target_os = "macos")]
        if let Some((origin, nudge)) = placed
            && set_frame_once(&window, target_frame(origin, next))
        {
            memory.set_nudge(nudge);
            memory.set_placed_at(Some(origin));
            current.set(next);
            return;
        }
        resize(&window, next);
        if let Some((origin, nudge)) = placed {
            let _ = window.set_position(Space::of_this_build().position(origin));
            memory.set_nudge(nudge);
            memory.set_placed_at(Some(origin));
        }
    }
    current.set(next);
}

/// An answer landed: the idle clock restarts, so the answer lives the cold
/// threshold from its arrival and not from the hide (owner, 2026-10-01). Asked
/// from a launcher that hid while the ask was in flight, the answer would
/// otherwise be dropped, with its query, by a show that is cold by the hide's
/// clock. Unconditional: marking while the launcher is visible is harmless,
/// because every hide (`hide_launcher`, the `Focused(false)` arm) marks again.
///
/// Hidden, it also arms a cold clock (`crate::answer_landed`): the mark voids
/// the ones armed at the hide, so nothing else would give the models back.
#[tauri::command]
pub fn launcher_answered<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager as _;
    let visible = app
        .get_webview_window("launcher")
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    match crate::cold_after(&app) {
        Some(after) => crate::answer_landed(&app, visible, after),
        None => app.state::<HiddenAt>().mark(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_is_the_visible_columns() {
        let l = |left, right| Layout { left, right };
        assert_eq!(width(l(false, false)), 470.0);
        assert_eq!(width(l(true, false)), 760.0);
        assert_eq!(width(l(false, true)), 841.0);
        assert_eq!(width(l(true, true)), 1131.0);
    }

    #[test]
    fn the_frame_is_the_given_origin_and_the_new_width() {
        use crate::launcher_position::Point;
        let l = |left, right| Layout { left, right };
        let origin = Point { x: 390, y: 50 };
        let f = target_frame(origin, l(false, true));
        assert_eq!(f.origin, origin);
        assert_eq!((f.width, f.height), (841.0, 592.0));
    }

    #[test]
    fn appkit_counts_up_from_the_primary_screens_bottom() {
        // A window whose top is 50 below a 900-high primary screen's top, 592
        // tall, has its bottom edge 258 above the primary screen's bottom.
        assert_eq!(appkit_origin_y(50.0, 592.0, 900.0), 258.0);
        // Above the primary screen (a monitor stacked over it): negative top.
        assert_eq!(appkit_origin_y(-700.0, 592.0, 900.0), 1008.0);
    }

    #[test]
    fn only_the_left_column_offsets_the_search_column() {
        assert_eq!(
            search_offset(Layout {
                left: false,
                right: true
            }),
            0.0
        );
        assert_eq!(
            search_offset(Layout {
                left: true,
                right: false
            }),
            290.0
        );
    }

    #[test]
    fn cold_exactly_at_the_threshold_not_a_second_before() {
        let t0 = SystemTime::now();
        assert!(goes_cold(Some(t0), t0 + Duration::from_secs(300), 5));
        assert!(!goes_cold(Some(t0), t0 + Duration::from_secs(299), 5));
        assert!(!goes_cold(None, t0 + Duration::from_secs(9999), 5));
    }

    #[test]
    fn a_clock_set_back_counts_as_elapsed() {
        let t0 = SystemTime::now();
        assert!(goes_cold(Some(t0), t0 - Duration::from_secs(3600), 5));
    }

    #[test]
    fn the_default_layout_is_cold() {
        assert_eq!(
            Layout::default(),
            Layout {
                left: false,
                right: false
            }
        );
    }
}
