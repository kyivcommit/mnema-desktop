//! The tray menu's icons (Task 1), replacing the emoji glyphs `tray::tray_label`
//! used to compose onto its text. Six fixed pictures — Status, Search,
//! Settings, Stop, Resume, Quit — sharing one logical size, one RGBA layout
//! and one neutral ink colour, so a menu row never mixes styles.
//!
//! No PNG decoder and no theme engine: each icon is a short list of analytic
//! shapes (a ring, a stroked segment, a disc, a rounded square, a triangle)
//! in a 1.0×1.0 unit square, rasterized by [`rasterize`] into RGBA bytes —
//! coverage from 4×4 sub-sampling, not a bitmap on disk. If a native surface
//! — light, dark, or a highlighted row — renders the result unreadably, the
//! fix is the one [`INK`] tuple below, not a second engine; native visibility
//! itself is Task 7's to check, not this one's (`build_tray_menu`/`Menu` are
//! macOS main-thread-only even under `cfg!(test)` — see `tray::TRAY_ITEM_IDS`'s
//! own doc for why no headless test can look at a rendered menu).
//!
//! Task 10: muda's `to_nsimage` fixes every menu-item icon's logical size at
//! 18 pt regardless of the bitmap's pixel size (`muda-0.19.3/src/
//! platform_impl/macos/{mod.rs:1164,icon.rs:41-60}`), so a small pixel grid
//! upscales visibly on Retina. Rendering at `SIZE` px and letting macOS
//! downsample gives anti-aliased edges instead.

use tauri::image::Image;

/// One picture per tray row that carries a icon (Task 1's six — the language
/// submenu's `CheckMenuItem`s stay bare until Task 3 removes them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuIcon {
    /// The status line's info circle.
    Status,
    /// «Показати пошук»'s magnifying glass.
    Search,
    /// «Налаштування»'s gear.
    Settings,
    /// «Зупинити сканування»'s square.
    Stop,
    /// «Продовжити сканування»'s triangle.
    Resume,
    /// «Вийти»'s power glyph.
    Quit,
}

/// The one grey every icon draws in, in every menu row — a shared style
/// rather than six independent choices, so a native rendering fix is one
/// tuple and not six.
const INK: (u8, u8, u8) = (122, 122, 122);

/// Bitmap side, in pixels; muda scales whatever this is to 18 pt (see the
/// module doc), so this is chosen for a crisp downsample, not to match it.
const SIZE: u32 = 72;

/// Sub-samples per axis per pixel; coverage is the fraction of the resulting
/// 16 sample points that land inside the icon's shapes.
const SUB: u32 = 4;

/// The one stroke thickness every ring, stem, handle and gear tooth draws
/// with, in unit-square terms — uniformity by construction, measured by
/// `menu_icons_share_one_stroke` rather than claimed in a comment.
const STROKE: f32 = 0.11;

/// A ring-bearing glyph's own centre, shared between its shape list below
/// and `menu_icons_share_one_stroke`'s ray sampling, so the two can't drift
/// apart the way the header-comment-vs-mask claim in this file once did.
const STATUS_CENTRE: (f32, f32) = (0.5, 0.5);
const SEARCH_CENTRE: (f32, f32) = (0.42, 0.42);
const SETTINGS_CENTRE: (f32, f32) = (0.5, 0.5);
const QUIT_CENTRE: (f32, f32) = (0.5, 0.46);

fn dist(p: (f32, f32), q: (f32, f32)) -> f32 {
    ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt()
}

/// Distance from `p` to the segment `a..b` — the basis of every capsule
/// (round-capped stroke) shape below.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let ab = (b.0 - a.0, b.1 - a.1);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    let t = if len2 > 0.0 {
        (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(p, (a.0 + t * ab.0, a.1 + t * ab.1))
}

/// One analytic shape in unit-square coordinates. A glyph is the union of a
/// short list of these — no shape carries its own colour, `INK` is the
/// whole picture's.
#[derive(Clone, Copy)]
enum Shape {
    /// An annulus at `r ± STROKE/2`, open on `[gap.0, gap.1)` degrees
    /// (measured from the centre, 0° = +x, clockwise since y grows down);
    /// `gap.0 == gap.1` (every ring but Quit's) means no gap at all.
    /// `gap.0 > gap.1` is a caller error, not a wraparound gap: no angle
    /// then satisfies `gap.0..gap.1`, so it silently draws a full ring too
    /// — `contains` debug-asserts against it instead of drawing it wrong.
    Ring {
        c: (f32, f32),
        r: f32,
        gap: (f32, f32),
    },
    /// A capsule: every point within `STROKE/2` of the segment `a..b`,
    /// round-capped — stems, the search handle, the gear's teeth.
    Stroke {
        a: (f32, f32),
        b: (f32, f32),
    },
    Disc {
        c: (f32, f32),
        r: f32,
    },
    /// An axis-aligned square of half-extent `half`, corners rounded to
    /// `corner` (the standard rounded-box signed-distance test).
    RoundedSquare {
        c: (f32, f32),
        half: f32,
        corner: f32,
    },
    Triangle {
        a: (f32, f32),
        b: (f32, f32),
        c: (f32, f32),
    },
}

impl Shape {
    fn contains(&self, p: (f32, f32)) -> bool {
        match *self {
            Shape::Ring { c, r, gap } => {
                debug_assert!(
                    gap.0 <= gap.1,
                    "Ring gap must be gap.0 <= gap.1, got {gap:?}"
                );
                if (dist(p, c) - r).abs() > STROKE / 2.0 {
                    return false;
                }
                if gap.0 == gap.1 {
                    return true;
                }
                let mut angle = (p.1 - c.1).atan2(p.0 - c.0).to_degrees();
                if angle < 0.0 {
                    angle += 360.0;
                }
                !(gap.0..gap.1).contains(&angle)
            }
            Shape::Stroke { a, b } => seg_dist(p, a, b) <= STROKE / 2.0,
            Shape::Disc { c, r } => dist(p, c) <= r,
            Shape::RoundedSquare { c, half, corner } => {
                let qx = (p.0 - c.0).abs() - (half - corner);
                let qy = (p.1 - c.1).abs() - (half - corner);
                let outside =
                    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0);
                outside <= corner
            }
            Shape::Triangle { a, b, c } => {
                fn side(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
                    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
                }
                let (d1, d2, d3) = (side(p, a, b), side(p, b, c), side(p, c, a));
                let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
                let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
                !(has_neg && has_pos)
            }
        }
    }
}

/// The shape list for one icon, in the 1.0×1.0 unit square. Every extra
/// feature on a ring glyph (Status's dot and stem, Search's handle,
/// Settings's teeth, Quit's stem) is placed clear of its ring's own
/// horizontal centre line, so `menu_icons_share_one_stroke` sees only the
/// ring on that ray — and, for Status specifically, clear of the ring band
/// itself by a margin `status_dot_and_stem_clear_the_ring` pins, so the dot
/// and stem read as separate from the ring rather than fused into it.
fn icon_shapes(icon: MenuIcon) -> Vec<Shape> {
    match icon {
        MenuIcon::Status => vec![
            Shape::Ring {
                c: STATUS_CENTRE,
                r: 0.30,
                gap: (0.0, 0.0),
            },
            Shape::Disc {
                c: (0.5, 0.345),
                r: STROKE / 2.0,
            },
            Shape::Stroke {
                a: (0.5, 0.585),
                b: (0.5, 0.655),
            },
        ],
        MenuIcon::Search => {
            let r = 0.22;
            let dir = (
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            );
            let start = (SEARCH_CENTRE.0 + r * dir.0, SEARCH_CENTRE.1 + r * dir.1);
            let end = (
                SEARCH_CENTRE.0 + (r + 0.28) * dir.0,
                SEARCH_CENTRE.1 + (r + 0.28) * dir.1,
            );
            vec![
                Shape::Ring {
                    c: SEARCH_CENTRE,
                    r,
                    gap: (0.0, 0.0),
                },
                Shape::Stroke { a: start, b: end },
            ]
        }
        MenuIcon::Settings => {
            let r = 0.24;
            let tooth = 0.14;
            let mut shapes = vec![Shape::Ring {
                c: SETTINGS_CENTRE,
                r,
                gap: (0.0, 0.0),
            }];
            for deg in [30.0_f32, 90.0, 150.0, 210.0, 270.0, 330.0] {
                let (sin, cos) = deg.to_radians().sin_cos();
                let a = (SETTINGS_CENTRE.0 + r * cos, SETTINGS_CENTRE.1 + r * sin);
                let b = (
                    SETTINGS_CENTRE.0 + (r + tooth) * cos,
                    SETTINGS_CENTRE.1 + (r + tooth) * sin,
                );
                shapes.push(Shape::Stroke { a, b });
            }
            shapes
        }
        MenuIcon::Stop => vec![Shape::RoundedSquare {
            c: (0.5, 0.5),
            half: 0.27,
            corner: 0.06,
        }],
        MenuIcon::Resume => vec![Shape::Triangle {
            a: (0.32, 0.22),
            b: (0.32, 0.78),
            c: (0.76, 0.5),
        }],
        MenuIcon::Quit => vec![
            Shape::Ring {
                c: QUIT_CENTRE,
                r: 0.22,
                gap: (245.0, 295.0),
            },
            Shape::Stroke {
                a: (0.5, 0.14),
                b: (0.5, 0.36),
            },
        ],
    }
}

/// Rasterizes a shape list into `SIZE × SIZE` RGBA bytes: each pixel's
/// alpha is its coverage (the fraction of `SUB × SUB` sample points inside
/// any shape) times 255, colour fixed at [`INK`].
fn rasterize(shapes: &[Shape]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let mut hits = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let x = (px as f32 + (sx as f32 + 0.5) / SUB as f32) / SIZE as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SUB as f32) / SIZE as f32;
                    if shapes.iter().any(|s| s.contains((x, y))) {
                        hits += 1;
                    }
                }
            }
            let total = SUB * SUB;
            let alpha = ((hits * 255 + total / 2) / total) as u8;
            pixels.extend_from_slice(&[INK.0, INK.1, INK.2, alpha]);
        }
    }
    pixels
}

/// The one call site every tray row that carries an icon goes through —
/// builds `icon`'s shape list and rasterizes it into an owned RGBA image.
pub(crate) fn menu_icon(icon: MenuIcon) -> Image<'static> {
    Image::new_owned(rasterize(&icon_shapes(icon)), SIZE, SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [MenuIcon; 6] = [
        MenuIcon::Status,
        MenuIcon::Search,
        MenuIcon::Settings,
        MenuIcon::Stop,
        MenuIcon::Resume,
        MenuIcon::Quit,
    ];

    fn alpha_at(bytes: &[u8], size: u32, x: u32, y: u32) -> u8 {
        bytes[((y * size + x) * 4 + 3) as usize]
    }

    /// Every icon: `SIZE`×`SIZE` RGBA, matching what [`Image::new_owned`]
    /// was actually built with — the rasterizer's whole contract at 72 px,
    /// where the 16×16 masks used to stop.
    #[test]
    fn menu_icons_are_72_px_rgba() {
        for icon in ALL {
            let image = menu_icon(icon);
            assert_eq!(image.width(), SIZE, "{icon:?} width");
            assert_eq!(image.height(), SIZE, "{icon:?} height");
            assert_eq!(
                image.rgba().len(),
                (4 * SIZE * SIZE) as usize,
                "{icon:?} rgba byte count"
            );
        }
    }

    /// A 1-bit mask has no pixel strictly between transparent and opaque;
    /// anti-aliasing is exactly that pixel existing, on every icon.
    #[test]
    fn menu_icons_are_anti_aliased() {
        for icon in ALL {
            let bytes = menu_icon(icon).rgba().to_vec();
            assert!(
                bytes.chunks(4).any(|p| p[3] > 0 && p[3] < 255),
                "{icon:?} has no partially-covered pixel"
            );
        }
    }

    /// Non-empty (at least one opaque pixel) and distinct from every other
    /// icon's bytes — an icon that came out blank, or two icons that came
    /// out identical, would both still satisfy "72×72 RGBA" without this.
    #[test]
    fn menu_icons_are_distinct_and_nonempty() {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for icon in ALL {
            let bytes = menu_icon(icon).rgba().to_vec();
            assert!(bytes.iter().any(|&b| b != 0), "{icon:?} drew nothing");
            assert!(
                !seen.iter().any(|other| other == &bytes),
                "{icon:?} draws the same picture as an earlier icon"
            );
            seen.push(bytes);
        }
    }

    /// The four ring glyphs draw the same stroke: sampling the horizontal
    /// ray through each ring's own centre and counting fully-opaque pixels
    /// gives the same count (±2 px — sub-pixel rounding measured as high as
    /// 1 px on its own) whichever ring it is — a property of one shared
    /// [`STROKE`], not four independent guesses.
    #[test]
    fn menu_icons_share_one_stroke() {
        let rows = [
            (MenuIcon::Status, STATUS_CENTRE.1),
            (MenuIcon::Search, SEARCH_CENTRE.1),
            (MenuIcon::Quit, QUIT_CENTRE.1),
            (MenuIcon::Settings, SETTINGS_CENTRE.1),
        ];
        let mut counts = Vec::new();
        for (icon, centre_y) in rows {
            let bytes = menu_icon(icon).rgba().to_vec();
            let row = (centre_y * SIZE as f32) as u32;
            let opaque = (0..SIZE)
                .filter(|&x| alpha_at(&bytes, SIZE, x, row) == 255)
                .count() as i32;
            counts.push((icon, opaque));
        }
        let base = counts[0].1;
        for (icon, count) in &counts {
            assert!(
                (*count - base).abs() <= 2,
                "{icon:?} ring stroke measured {count} px, expected {base} ±2"
            );
        }
    }

    /// The Status glyph's dot and stem sit inside the ring without fusing
    /// into it: sampling the vertical centre column finds exactly four
    /// fully-opaque runs top to bottom (the ring's own top arc, the dot,
    /// the stem, the ring's own bottom arc), each bordered by at least one
    /// fully transparent pixel — a dot or stem that crept into the ring
    /// band would merge two of these into one run instead.
    #[test]
    fn status_dot_and_stem_clear_the_ring() {
        let bytes = menu_icon(MenuIcon::Status).rgba().to_vec();
        let col = (STATUS_CENTRE.0 * SIZE as f32) as u32;
        let alphas: Vec<u8> = (0..SIZE).map(|y| alpha_at(&bytes, SIZE, col, y)).collect();

        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        for (y, &a) in alphas.iter().enumerate() {
            if a == 255 {
                start.get_or_insert(y);
            } else if let Some(s) = start.take() {
                runs.push((s, y - 1));
            }
        }
        if let Some(s) = start {
            runs.push((s, alphas.len() - 1));
        }

        assert_eq!(
            runs.len(),
            4,
            "expected ring-top, dot, stem, ring-bottom opaque runs, got {runs:?}"
        );
        for pair in runs.windows(2) {
            let (_, end) = pair[0];
            let (next_start, _) = pair[1];
            assert!(
                (end + 1..next_start).any(|y| alphas[y] == 0),
                "runs {:?} and {:?} are not separated by a fully transparent pixel",
                pair[0],
                pair[1]
            );
        }
    }

    /// No shape reaches the outer 2-px border, so macOS's downsampling to
    /// 18 pt never clips a glyph against the canvas edge.
    #[test]
    fn menu_icons_fit_the_canvas() {
        for icon in ALL {
            let bytes = menu_icon(icon).rgba().to_vec();
            for y in 0..SIZE {
                for x in 0..SIZE {
                    if !(2..SIZE - 2).contains(&x) || !(2..SIZE - 2).contains(&y) {
                        assert_eq!(
                            alpha_at(&bytes, SIZE, x, y),
                            0,
                            "{icon:?} opaque pixel at border ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}
