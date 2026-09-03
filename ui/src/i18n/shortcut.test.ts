import { expect, test } from 'vitest';
import { formatShortcut, isModifierOnlyPress, shortcutFromEvent } from './shortcut';

// A `keydown` as the recorder receives it. `code` is the physical key and is
// what the mapping reads; `key` is carried too because the cancel path and the
// modifier-only check are the two places where the LOGICAL key is the honest
// question ("did the person press Escape", "is this press nothing but a
// modifier"), and a fixture that left it undefined would be describing an event
// no browser sends.
const press = (over: Partial<KeyboardEventInit> = {}): KeyboardEvent =>
  new KeyboardEvent('keydown', { key: 'a', code: 'KeyA', ...over });

// ---------------------------------------------------------------------------
// `formatShortcut` — the same stored string, on all three platforms.
//
// Three, and not "the one this test runs on": `Platform` is chosen at compile
// time on the Rust side (`Platform::of_this_build`) and crosses the wire, so a
// window built on Linux renders a mac string whenever a mac build sends one.
// A suite that only ever asked about its own platform would pass on a formatter
// that had no other branch at all.
// ---------------------------------------------------------------------------

test('the default shortcut is drawn with mac glyphs on mac and with words elsewhere', () => {
  expect(formatShortcut('Alt+Space', 'mac')).toBe('⌥Space');
  expect(formatShortcut('Alt+Space', 'windows')).toBe('Alt+Space');
  expect(formatShortcut('Alt+Space', 'linux')).toBe('Alt+Space');
  // Both directions: the mac form is not merely "also" glyphs — the word is
  // gone, and so is the separator a person would otherwise read as a key.
  expect(formatShortcut('Alt+Space', 'mac')).not.toContain('Alt');
  expect(formatShortcut('Alt+Space', 'mac')).not.toContain('+');
});

test('all four modifiers are drawn in the canonical order, and Super is spelled per platform', () => {
  expect(formatShortcut('Ctrl+Alt+Shift+Super+A', 'mac')).toBe('⌃⌥⇧⌘A');
  expect(formatShortcut('Ctrl+Alt+Shift+Super+A', 'windows')).toBe('Ctrl+Alt+Shift+Win+A');
  expect(formatShortcut('Ctrl+Alt+Shift+Super+A', 'linux')).toBe('Ctrl+Alt+Shift+Super+A');
  // The three platforms disagree, positively — a formatter that ignored its
  // second argument would satisfy any one of the three assertions above.
  expect(formatShortcut('Ctrl+Alt+Shift+Super+A', 'windows'))
    .not.toBe(formatShortcut('Ctrl+Alt+Shift+Super+A', 'linux'));
  expect(formatShortcut('Ctrl+Alt+Shift+Super+A', 'mac'))
    .not.toBe(formatShortcut('Ctrl+Alt+Shift+Super+A', 'linux'));
});

// The window builds its strings canonically, but the STORE need not hold one it
// built: `set_hotkey` accepts any order the parser accepts as long as the key
// comes last (`global-hotkey`'s `hotkey.rs` refuses only a key in the middle),
// and `prefs.json` is a file on the person's own disk. Two spellings of one
// shortcut must not read as two different shortcuts.
test('a stored shortcut whose modifiers are in another order is still drawn canonically', () => {
  expect(formatShortcut('Super+Shift+Alt+Ctrl+A', 'mac')).toBe('⌃⌥⇧⌘A');
  expect(formatShortcut('Super+Shift+Alt+Ctrl+A', 'linux')).toBe('Ctrl+Alt+Shift+Super+A');
});

// ---------------------------------------------------------------------------
// `shortcutFromEvent` — what the recorder is allowed to build a string from.
// ---------------------------------------------------------------------------

test('a press with no modifier at all builds nothing', () => {
  // `Space` alone parses on the Rust side and would take the space bar away
  // system-wide (D-b step 3), so the refusal exists on both sides and this is
  // this side's.
  expect(shortcutFromEvent(press({ key: ' ', code: 'Space' }))).toBeNull();
  expect(shortcutFromEvent(press())).toBeNull();
  // Both directions: the very same key WITH a modifier does build a string, so
  // the null above is about the missing modifier and not about the key.
  expect(shortcutFromEvent(press({ key: ' ', code: 'Space', altKey: true }))).toBe('Alt+Space');
});

test('a press that is nothing but a modifier is ignored rather than refused', () => {
  const modifiers: [string, string, Partial<KeyboardEventInit>][] = [
    ['Control', 'ControlLeft', { ctrlKey: true }],
    ['Alt', 'AltLeft', { altKey: true }],
    ['Shift', 'ShiftRight', { shiftKey: true }],
    ['Meta', 'MetaLeft', { metaKey: true }],
  ];
  for (const [key, code, flags] of modifiers) {
    const e = press({ key, code, ...flags });
    expect(shortcutFromEvent(e)).toBeNull();
    // 🔴 The two nulls are not the same null, and the section says two
    // different things about them: holding Ctrl on the way to Ctrl+A must not
    // put a refusal on the screen, while an unusable key must.
    expect(isModifierOnlyPress(e)).toBe(true);
  }
  // Both directions: an ordinary press is not a modifier-only one.
  expect(isModifierOnlyPress(press({ ctrlKey: true }))).toBe(false);
});

test('Escape builds nothing and is not a modifier-only press, so the section can cancel on it', () => {
  const bare = press({ key: 'Escape', code: 'Escape' });
  expect(shortcutFromEvent(bare)).toBeNull();
  expect(isModifierOnlyPress(bare)).toBe(false);
  // Held with a modifier it is still not bindable: the recorder's cancel is the
  // only meaning this key has here, and a Ctrl+Escape a person could store
  // would take the cancel away from the next recording.
  expect(shortcutFromEvent(press({ key: 'Escape', code: 'Escape', ctrlKey: true }))).toBeNull();
});

test('the modifiers are emitted in the canonical order, whichever way the event states them', () => {
  // Alt named first in the initialiser, Ctrl second — the string is still
  // `Ctrl+Alt+A`. The event carries four booleans and no order at all, so the
  // order can only come from this module.
  expect(shortcutFromEvent(press({ altKey: true, ctrlKey: true }))).toBe('Ctrl+Alt+A');
  expect(shortcutFromEvent(press({ ctrlKey: true, altKey: true }))).toBe('Ctrl+Alt+A');
  expect(shortcutFromEvent(press({ metaKey: true, shiftKey: true, altKey: true, ctrlKey: true })))
    .toBe('Ctrl+Alt+Shift+Super+A');
  // The command key crosses as `Super`, which is the spelling the Rust parser
  // takes on every platform.
  expect(shortcutFromEvent(press({ metaKey: true }))).toBe('Super+A');
});

test('the codes the map carries become the plugin key names', () => {
  expect(shortcutFromEvent(press({ key: 'z', code: 'KeyZ', ctrlKey: true }))).toBe('Ctrl+Z');
  expect(shortcutFromEvent(press({ key: '1', code: 'Digit1', ctrlKey: true }))).toBe('Ctrl+1');
  expect(shortcutFromEvent(press({ key: ' ', code: 'Space', altKey: true }))).toBe('Alt+Space');
  expect(shortcutFromEvent(press({ key: 'F1', code: 'F1', ctrlKey: true }))).toBe('Ctrl+F1');
  expect(shortcutFromEvent(press({ key: 'F12', code: 'F12', ctrlKey: true }))).toBe('Ctrl+F12');
  expect(shortcutFromEvent(press({ key: 'ArrowUp', code: 'ArrowUp', altKey: true }))).toBe('Alt+ArrowUp');
  expect(shortcutFromEvent(press({ key: 'ArrowLeft', code: 'ArrowLeft', altKey: true }))).toBe('Alt+ArrowLeft');
});

test('a code the map does not carry builds nothing, and is not read as a modifier-only press', () => {
  // A dead key on a French layout, and a key no map here has heard of. Both
  // must come back null rather than as `Ctrl+IntlBackslash`, which the Rust
  // parser would then refuse with a sentence asking the person to open a
  // GitHub issue.
  for (const code of ['IntlBackslash', 'Lang1', 'BrowserSearch', 'Unidentified']) {
    const e = press({ key: 'x', code, ctrlKey: true });
    expect(shortcutFromEvent(e)).toBeNull();
    // Not silently swallowed: the section owes this press a sentence, and it
    // tells it from a held modifier by exactly this answer.
    expect(isModifierOnlyPress(e)).toBe(false);
  }
});
