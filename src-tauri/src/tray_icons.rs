//! The tray menu's icons (Task 1), replacing the emoji glyphs `tray::tray_label`
//! used to compose onto its text. Six fixed pictures — Status, Search,
//! Settings, Stop, Resume, Quit — sharing one logical size, one RGBA layout
//! and one neutral ink colour, so a menu row never mixes styles.
//!
//! No PNG decoder and no theme engine: each mask is a hand-drawn `[u16; 16]`
//! bitmap (one bit per pixel, MSB = leftmost column) turned into RGBA bytes by
//! [`rgba`]'s straight iteration. If a native surface — light, dark, or a
//! highlighted row — renders the result unreadably, the fix is the one [`INK`]
//! tuple below, not a second engine; native visibility itself is Task 7's to
//! check, not this one's (`build_tray_menu`/`Menu` are macOS main-thread-only
//! even under `cfg!(test)` — see `tray::TRAY_ITEM_IDS`'s own doc for why no
//! headless test can look at a rendered menu).

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

/// Turns a 16-row bitmap into the RGBA bytes [`tauri::image::Image::
/// new_owned`] wants: a straight per-pixel iteration, bit 15 first so it lands
/// on the leftmost pixel — no PNG decoder anywhere in this module.
fn rgba(rows: [u16; 16]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for row in rows {
        for bit in (0..16).rev() {
            let alpha = if row & (1 << bit) == 0 { 0 } else { 255 };
            pixels.extend_from_slice(&[INK.0, INK.1, INK.2, alpha]);
        }
    }
    pixels
}

// Each mask below is 16 rows of 16 bits, MSB-first (leftmost pixel first),
// generated from the ASCII art in its comment so the drawing and the bits
// can be checked against each other at a glance.

/// ```text
/// .....######.....
/// ...###.##.###...
/// ...#...##...#...
/// ..##........##..
/// ..#....##....#..
/// ..#....##....#..
/// ..#....##....#..
/// ..#....##....#..
/// ..##...##...##..
/// ...#...##...#...
/// ...###....###...
/// .....######.....
/// ```
const STATUS: [u16; 16] = [
    0b0000000000000000,
    0b0000000000000000,
    0b0000011111100000,
    0b0001110110111000,
    0b0001000110001000,
    0b0011000000001100,
    0b0010000110000100,
    0b0010000110000100,
    0b0010000110000100,
    0b0010000110000100,
    0b0011000110001100,
    0b0001000110001000,
    0b0001110000111000,
    0b0000011111100000,
    0b0000000000000000,
    0b0000000000000000,
];

/// ```text
/// ....#####.......
/// ...#######......
/// ..##.....##.....
/// ..##.....##.....
/// ..##.....##.....
/// ..##.....##.....
/// ..##.....##.....
/// ...########.....
/// ....#####.##....
/// ...........##...
/// ............##..
/// .............##.
/// ```
const SEARCH: [u16; 16] = [
    0b0000000000000000,
    0b0000000000000000,
    0b0000111110000000,
    0b0001111111000000,
    0b0011000001100000,
    0b0011000001100000,
    0b0011000001100000,
    0b0011000001100000,
    0b0011000001100000,
    0b0001111111100000,
    0b0000111110110000,
    0b0000000000011000,
    0b0000000000001100,
    0b0000000000000110,
    0b0000000000000000,
    0b0000000000000000,
];

/// ```text
/// .......#........
/// .......#........
/// ...#.######.#...
/// ....##....##....
/// ...##......##...
/// ...#........#...
/// ...#........#...
/// .###........#.#.
/// ...#........#...
/// ...##......##...
/// ....##....##....
/// ...#.######.#...
/// ........#.......
/// ```
const SETTINGS: [u16; 16] = [
    0b0000000000000000,
    0b0000000100000000,
    0b0000000100000000,
    0b0001011111101000,
    0b0000110000110000,
    0b0001100000011000,
    0b0001000000001000,
    0b0001000000001000,
    0b0111000000001010,
    0b0001000000001000,
    0b0001100000011000,
    0b0000110000110000,
    0b0001011111101000,
    0b0000000000000000,
    0b0000000010000000,
    0b0000000000000000,
];

/// A solid square — Stop needs no outline, unlike its neighbours.
const STOP: [u16; 16] = [
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000011111100000,
    0b0000011111100000,
    0b0000011111100000,
    0b0000011111100000,
    0b0000011111100000,
    0b0000011111100000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
];

/// A right-pointing (play) triangle.
const RESUME: [u16; 16] = [
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000010000000000,
    0b0000011100000000,
    0b0000011111000000,
    0b0000011111110000,
    0b0000011111110000,
    0b0000011111000000,
    0b0000011100000000,
    0b0000010000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
    0b0000000000000000,
];

/// ```text
/// .......##.......
/// .......##.......
/// .......##.......
/// ....#..##..#....
/// ...##..##..##...
/// ...#...##...#...
/// ..##...##...##..
/// ..##...##...##..
/// ..##........##..
/// ..##........##..
/// ...#........#...
/// ...##......##...
/// ....########....
/// ......####......
/// ```
const QUIT: [u16; 16] = [
    0b0000000000000000,
    0b0000000110000000,
    0b0000000110000000,
    0b0000000110000000,
    0b0000100110010000,
    0b0001100110011000,
    0b0001000110001000,
    0b0011000110001100,
    0b0011000110001100,
    0b0011000000001100,
    0b0011000000001100,
    0b0001000000001000,
    0b0001100000011000,
    0b0000111111110000,
    0b0000001111000000,
    0b0000000000000000,
];

/// The one call site every tray row that carries an icon goes through —
/// picks the fixed mask for `icon` and turns it into an owned RGBA image.
pub(crate) fn menu_icon(icon: MenuIcon) -> Image<'static> {
    let mask = match icon {
        MenuIcon::Status => STATUS,
        MenuIcon::Search => SEARCH,
        MenuIcon::Settings => SETTINGS,
        MenuIcon::Stop => STOP,
        MenuIcon::Resume => RESUME,
        MenuIcon::Quit => QUIT,
    };
    Image::new_owned(rgba(mask), 16, 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon: 16×16, RGBA (1024 bytes = 16*16*4), a non-empty mask (at
    /// least one opaque pixel), and distinct from every other icon's bytes —
    /// a mask that came out blank, or two icons that came out identical,
    /// would both still satisfy "16×16 RGBA" without this last check.
    #[test]
    fn menu_icons_have_equal_nonempty_bounds() {
        let all = [
            MenuIcon::Status,
            MenuIcon::Search,
            MenuIcon::Settings,
            MenuIcon::Stop,
            MenuIcon::Resume,
            MenuIcon::Quit,
        ];
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for icon in all {
            let image = menu_icon(icon);
            assert_eq!(image.width(), 16, "{icon:?} width");
            assert_eq!(image.height(), 16, "{icon:?} height");
            let bytes = image.rgba();
            assert_eq!(bytes.len(), 16 * 16 * 4, "{icon:?} rgba byte count");
            assert!(
                bytes.iter().any(|&b| b != 0),
                "{icon:?} mask drew nothing"
            );
            assert!(
                !seen.iter().any(|other| other == bytes),
                "{icon:?} draws the same picture as an earlier icon"
            );
            seen.push(bytes.to_vec());
        }
    }
}
