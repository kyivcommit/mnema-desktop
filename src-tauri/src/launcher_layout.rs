//! The launcher's width and where its search column sits in it. The numbers are
//! declarations in `ui/src/styles/launcher.css` (`main.panels`); the guard
//! `the handle offset matches the stylesheet` (`Launcher.test.ts`) reads both.
use std::sync::Mutex;

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
