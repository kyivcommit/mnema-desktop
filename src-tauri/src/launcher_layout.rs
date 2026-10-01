//! The launcher's width and where its search column sits in it. The numbers are
//! declarations in `ui/src/styles/launcher.css` (`main.panels`); the guard
//! `the handle offset matches the stylesheet` (`Launcher.test.ts`) reads both.
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
/// because the tracks plus gaps exactly fill the window's content box, so
/// `justify-content: center` and the minmax floor never engage; if either
/// did, the column would sit elsewhere and this offset would be wrong.
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
/// the show.
#[derive(Default)]
pub struct HiddenAt(Mutex<Option<Instant>>);

impl HiddenAt {
    pub fn mark(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
    pub fn get(&self) -> Option<Instant> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Whether a show at `now` finds the launcher cold: hidden for at least
/// `minutes`. Never hidden (`None`) is not cold: the start-up layout already is.
pub fn goes_cold(hidden_at: Option<Instant>, now: Instant, minutes: u32) -> bool {
    hidden_at.is_some_and(|t| {
        now.saturating_duration_since(t) >= Duration::from_secs(u64::from(minutes) * 60)
    })
}

/// Sets the window to the size of layout `l`. The one place that turns a
/// layout into a window size, for `set_launcher_layout` and the cold show.
pub fn resize<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, l: Layout) {
    let _ = window.set_size(tauri::LogicalSize::new(width(l), HEIGHT));
}

/// Resizes the launcher to its visible panels; the search column keeps its
/// place on screen (owner, 2026-09-25). Synchronous: AppKit wants the window
/// size from the main thread, as `open_settings`.
#[tauri::command]
pub fn set_launcher_layout<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    current: tauri::State<'_, Current>,
    left: bool,
    right: bool,
) {
    use crate::launcher_position::{Space, relayout, window_corner};
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
        resize(&window, next);
        if let Some((c, factor)) = corner {
            let _ = window
                .set_position(Space::of_this_build().position(relayout(c, before, next, factor)));
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
#[tauri::command]
pub fn launcher_answered(hidden_at: tauri::State<'_, HiddenAt>) {
    hidden_at.mark();
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
        let t0 = Instant::now();
        assert!(goes_cold(Some(t0), t0 + Duration::from_secs(300), 5));
        assert!(!goes_cold(Some(t0), t0 + Duration::from_secs(299), 5));
        assert!(!goes_cold(None, t0 + Duration::from_secs(9999), 5));
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
