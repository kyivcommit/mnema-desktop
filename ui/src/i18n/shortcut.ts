import type { Platform } from '../lib/ipc';

// The global shortcut, in both directions: the string the backend stores turned
// into something a person reads, and a keypress turned back into that string.
//
// 🔴 **Why this module is filed under `src/i18n/` — the reason is filing, not
// the guard.** Every `.ts` file anywhere under `ui/src` is already outside the
// F3 Latin sweep by construction (`guard.test.ts` walks `.svelte` files only),
// so this directory buys nothing there and a reader must not take it for a way
// of skipping a check. What it is: a locale-shaped mapping module. It turns one
// stored value into the words and glyphs of the platform a person is on, which
// is what `recency.ts` and the catalogue beside it do, and both halves are pure
// functions unit-tested with no component in sight.
//
// ⚠️ The strings below are `global-hotkey`'s own vocabulary — `Ctrl`, `Alt`,
// `Shift`, `Super`, `Space`, `F1`, `ArrowUp` — and protocol tokens rather than
// prose, exactly as `prefs.rs` says of `MODIFIER_SPELLINGS` on the Rust side.
// Nothing here is a sentence; every sentence this section shows comes from
// `catalog.ts` or verbatim from the backend.

// The canonical order, and it is canonical rather than incidental. The parser
// is indifferent to the order of the modifiers AMONG THEMSELVES but not to the
// key, which must come last; what one fixed order buys is that two people who
// press the same keys store the same string and read the same label.
const ORDER = ['Ctrl', 'Alt', 'Shift', 'Super'] as const;
type Modifier = (typeof ORDER)[number];

// ⌃⌥⇧⌘ — the order Apple prints them in, which is the order above.
const MAC_GLYPH: Record<Modifier, string> = {
  Ctrl: '⌃',
  Alt: '⌥',
  Shift: '⇧',
  Super: '⌘',
};

// How the command/meta key is written where it is not a glyph. Linux keeps the
// parser's own spelling; Windows says `Win`, which is what is printed on the
// key a person is looking at.
const SUPER_WORD: Record<Exclude<Platform, 'mac'>, string> = {
  windows: 'Win',
  linux: 'Super',
};

// Every spelling `global-hotkey` accepts for a modifier, folded onto the one
// this module emits. A stored string need not be one this window built —
// `prefs.json` is a file on the person's own disk, and the parser accepts any
// order of modifiers as long as the key is last — so two spellings of one
// shortcut must not read as two different shortcuts.
const MODIFIER_ALIASES: Record<string, Modifier> = {
  ctrl: 'Ctrl', control: 'Ctrl', ctl: 'Ctrl',
  alt: 'Alt', option: 'Alt', altgr: 'Alt',
  shift: 'Shift',
  super: 'Super', meta: 'Super', cmd: 'Super', command: 'Super', win: 'Super',
};

/**
 * The stored shortcut, as the person on `platform` reads it.
 *
 * `platform` comes from the WIRE (`AppPrefs.platform`, chosen at compile time by
 * `Platform::of_this_build`) and never from `navigator.userAgent` — that type's
 * own doc records this project measuring a plausible proxy wrong twice, on two
 * platforms.
 *
 * Unknown tokens are passed through as they are rather than dropped: a string
 * this module does not fully understand is still the string the operating
 * system is holding, and showing less of it than there is would be the one
 * mistake worse than showing it awkwardly.
 */
export function formatShortcut(shortcut: string, platform: Platform): string {
  const tokens = shortcut.split('+');
  const key = tokens[tokens.length - 1] ?? '';
  const held = new Set<Modifier>();
  const unknown: string[] = [];
  for (const token of tokens.slice(0, -1)) {
    const known = MODIFIER_ALIASES[token.toLowerCase()];
    if (known) held.add(known);
    else unknown.push(token);
  }
  const modifiers = ORDER.filter((m) => held.has(m));

  if (platform === 'mac') {
    // No separator at all: `⌥Space` is how the combination is written on this
    // platform, and a `+` between glyphs reads as a key of its own.
    return modifiers.map((m) => MAC_GLYPH[m]).join('') + [...unknown, key].join('+');
  }
  const words = modifiers.map((m) => (m === 'Super' ? SUPER_WORD[platform] : m));
  return [...words, ...unknown, key].join('+');
}

// The logical keys a press consists of nothing but. Read from `key` rather than
// from `code`, because `key` is where "this press carries no character" is
// stated; the codes are listed beside it so a keyboard that reports one without
// the other is still understood.
const MODIFIER_KEYS = new Set(['Control', 'Alt', 'AltGraph', 'Shift', 'Meta', 'OS', 'CapsLock']);
const MODIFIER_CODES = new Set([
  'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight',
  'ShiftLeft', 'ShiftRight', 'MetaLeft', 'MetaRight', 'CapsLock',
]);

/**
 * Whether this press is a modifier and nothing else.
 *
 * 🔴 The reason this is exported beside [`shortcutFromEvent`] rather than folded
 * into it: both come back `null` from that function, and the section says two
 * different things about them. Holding Ctrl on the way to Ctrl+A must not put a
 * refusal on the screen — the press is ignored, and the recording goes on. A
 * key the map does not carry must, or a person presses it, sees nothing happen,
 * and has no way to learn why.
 */
export function isModifierOnlyPress(e: KeyboardEvent): boolean {
  return MODIFIER_KEYS.has(e.key) || MODIFIER_CODES.has(e.code);
}

// `event.code` — the physical key — folded onto the plugin's key names. Read
// from `code` and never from `key`, so that a shortcut recorded on one keyboard
// layout is the same physical combination on another, and so that Shift does
// not turn `1` into `!` halfway through building a string.
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  if (/^Arrow(Up|Down|Left|Right)$/.test(code)) return code;
  if (code === 'Space') return 'Space';
  return null;
}

/**
 * The string to send to `set_hotkey`, or `null` when this press cannot make one.
 *
 * Three refusals, and each of them has to be here rather than left to the
 * backend:
 *
 *   - a press that is nothing but a modifier — there is no key yet;
 *   - a press with no modifier at all — `Space` alone parses on the Rust side
 *     and would take the space bar away system-wide, which is why the guard
 *     exists on both sides (D-b step 3);
 *   - a key this map does not carry — the parser refuses those with a sentence
 *     asking the reader to report the string on GitHub, and handing that to a
 *     person who pressed a dead key would be this window's fault, not theirs.
 *
 * `Escape` is deliberately in none of the maps: it is the recorder's cancel, and
 * a `Ctrl+Escape` a person could store would take that cancel away from every
 * later recording.
 */
export function shortcutFromEvent(e: KeyboardEvent): string | null {
  if (isModifierOnlyPress(e)) return null;
  if (e.key === 'Escape' || e.code === 'Escape') return null;

  const held: Modifier[] = [];
  if (e.ctrlKey) held.push('Ctrl');
  if (e.altKey) held.push('Alt');
  if (e.shiftKey) held.push('Shift');
  if (e.metaKey) held.push('Super');
  if (held.length === 0) return null;

  const key = keyName(e.code);
  if (key === null) return null;

  // Built from ORDER rather than from the sequence the flags were read in: the
  // event carries four booleans and no order at all, so the order can only come
  // from this module, and it must be the same one every time.
  return [...ORDER.filter((m) => held.includes(m)), key].join('+');
}
