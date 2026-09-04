import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { tick } from 'svelte';
import Application from './Application.svelte';
import Settings from './Settings.svelte';
import { setLocale } from '../i18n';
import type { AppPrefs } from '../lib/ipc';

// The typed wrappers, not the raw `invoke` — the shape `Indexing.test.ts` uses.
// Every wrapper `Settings.svelte`'s other sections reach for is declared too,
// because the whole-window case at the bottom mounts all four: a wrapper left
// out is `undefined`, and every call on it becomes a TypeError a `catch`
// swallows.
const appPrefs = vi.fn();
const setHotkey = vi.fn();
const setAutostart = vi.fn();
const modelSettings = vi.fn();
const providerModels = vi.fn();
const listTree = vi.fn();
const listMasks = vi.fn();
const jobStatus = vi.fn();
vi.mock('../lib/ipc', () => ({
  appPrefs: (...a: unknown[]) => appPrefs(...a),
  setHotkey: (...a: unknown[]) => setHotkey(...a),
  setAutostart: (...a: unknown[]) => setAutostart(...a),
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  providerModels: (...a: unknown[]) => providerModels(...a),
  listTree: (...a: unknown[]) => listTree(...a),
  listMasks: (...a: unknown[]) => listMasks(...a),
  jobStatus: (...a: unknown[]) => jobStatus(...a),
  setKey: vi.fn(),
  forgetKey: vi.fn(),
  setChatModel: vi.fn(),
  setEmbeddingModel: vi.fn(),
  maskPreview: vi.fn(),
  addMask: vi.fn(),
  removeMask: vi.fn(),
  addWatchedFolder: vi.fn(),
  removeWatchedFolder: vi.fn(),
  startWalkJob: vi.fn(),
  startEmbedJob: vi.fn(),
  cancelJob: vi.fn(),
}));

// 🔴 Annotated `AppPrefs`, for the reason `Indexing.test.ts` annotates
// `ModelSettings`: every inline fixture in this project's UI suites sits behind
// an untyped mock where the compiler never looks, so a fixture that forgets
// `platform` or `version` would render `undefined` in front of a person and
// pass. Annotated, it is an `npm run check` error instead.
function prefs(over: Partial<AppPrefs> = {}): AppPrefs {
  return {
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'disabled' },
    version: '0.0.0',
    platform: 'linux',
    ...over,
  };
}

beforeEach(() => {
  appPrefs.mockReset();
  setHotkey.mockReset();
  setAutostart.mockReset();
  modelSettings.mockReset();
  providerModels.mockReset();
  listTree.mockReset();
  listMasks.mockReset();
  jobStatus.mockReset();
  appPrefs.mockResolvedValue(prefs());
  modelSettings.mockResolvedValue({
    key: { kind: 'present' },
    index: {
      kind: 'read', embeddingModel: null, chatModel: null,
      embeddedChunks: 0, embeddedChunksEverywhere: 0, totalChunks: 0,
      failedChunks: 0, indexedFiles: 0, lastIndexedAt: null,
      searchTextArm: true, searchContentArm: true,
    },
    platform: 'linux',
  });
  providerModels.mockResolvedValue({ entries: [], unreadable: 0, unreadableRecords: [] });
  listTree.mockResolvedValue({ roots: [], recents: [] });
  listMasks.mockResolvedValue([]);
  jobStatus.mockResolvedValue({ running: false });
  setLocale('uk');
});

afterEach(() => {
  cleanup();
  setLocale('en');
});

// What a person reads, with the markup's own indentation collapsed the way a
// browser collapses it.
const visible = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
const pageText = () => visible(document.body);
const at = (id: string) => visible(screen.getByTestId(id));

const renderSection = () => render(Application);

const shown = async (id: string) => {
  await waitFor(() => expect(screen.getByTestId(id)).toBeTruthy());
  return at(id);
};

// The recorder is driven the way a person drives it: press the control, then
// press keys at it. `code` is what the mapping reads, `key` is what the cancel
// path and the modifier-only check read, so both are always stated.
//
// 🔴 (review, Important 1) `record()` asserts focus, and `pressKey` dispatches
// on `document.activeElement` rather than on the testid directly. A real
// keydown reaches only whatever the DOM currently has focused — on macOS
// WebKit (Tauri's WKWebView) a click does NOT focus a `<button>`, so a suite
// that dispatched straight at the testid element would pass against a
// component no keypress on that platform could ever drive. Routing through
// `activeElement` makes every fixture below prove the real path, not just the
// callback.
const record = async () => {
  await shown('application-shortcut-record');
  const button = screen.getByTestId('application-shortcut-record');
  await fireEvent.click(button);
  await tick();
  expect(document.activeElement).toBe(button);
};
const pressKey = async (init: KeyboardEventInit) => {
  await fireEvent.keyDown(document.activeElement!, init);
  await tick();
};

const deferred = <T,>() => {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
};

// ---------------------------------------------------------------------------
// The shortcut, as the operating system reports it.
// ---------------------------------------------------------------------------

test('a registered shortcut is shown, and the sentence says registered rather than that it works', async () => {
  appPrefs.mockResolvedValue(prefs());

  renderSection();

  expect(await shown('application-shortcut')).toBe('Alt+Space');
  expect(await shown('application-shortcut-status')).toBe('Це скорочення зареєстровано в системі.');
  // 🔴 D128 measured macOS co-registering a shortcut another application
  // already holds: both register, both fire. So the sentence may not claim the
  // shortcut works, and may not claim it is this application's alone.
  expect(pageText()).not.toContain('працює');
  expect(pageText()).not.toContain('лише');
  // Nothing degraded is on the screen: no reason, and no way-out sentence.
  expect(screen.queryByTestId('application-shortcut-reason')).toBeNull();
  expect(screen.queryByTestId('application-shortcut-tray')).toBeNull();
});

test('an unavailable shortcut shows the backend reason verbatim and says the search can still be opened', async () => {
  const REASON = 'HotKey already registered by another application';
  appPrefs.mockResolvedValue(prefs({
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'unavailable', reason: REASON } },
  }));

  renderSection();

  expect(await shown('application-shortcut-status'))
    .toBe('Це скорочення не зареєстровано в системі.');
  // Verbatim, and English: a refusal the BACKEND makes is shown as it came.
  expect(at('application-shortcut-reason')).toContain(REASON);
  // A degraded state that offers no way forward is the state a person files a
  // bug about.
  expect(at('application-shortcut-tray'))
    .toBe('Пошук усе одно можна відкрити з піктограми застосунку в системному лотку.');
  // The shortcut itself is still drawn: it is what the person would change.
  expect(at('application-shortcut')).toBe('Alt+Space');
});

test('the two shortcut states get two sentences, not one drawn twice', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  const registered = await shown('application-shortcut-status');
  cleanup();

  appPrefs.mockResolvedValue(prefs({
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'unavailable', reason: 'taken' } },
  }));
  renderSection();
  const unavailable = await shown('application-shortcut-status');

  expect(registered).not.toBe(unavailable);
});

// ---------------------------------------------------------------------------
// The platform comes from the WIRE, never from the browser.
// ---------------------------------------------------------------------------

test('the same stored shortcut is drawn per the platform the reply states', async () => {
  const drawn: Record<string, string> = {};
  for (const platform of ['mac', 'windows', 'linux'] as const) {
    appPrefs.mockResolvedValue(prefs({
      platform,
      hotkey: { shortcut: 'Ctrl+Alt+Shift+Super+A', status: { kind: 'registered' } },
    }));
    renderSection();
    drawn[platform] = await shown('application-shortcut');
    cleanup();
  }

  expect(drawn.mac).toBe('⌃⌥⇧⌘A');
  expect(drawn.windows).toBe('Ctrl+Alt+Shift+Win+A');
  expect(drawn.linux).toBe('Ctrl+Alt+Shift+Super+A');
});

test('a mac reply is drawn with mac glyphs even though this window is not running on a mac', async () => {
  // 🔴 The one that names the hazard. `Platform` is chosen at compile time on
  // the Rust side and crosses the wire; a section reading `navigator.userAgent`
  // instead would draw `Alt+Space` here, because the test environment is not a
  // mac and neither is CI.
  expect(navigator.userAgent).not.toContain('Mac OS X');
  appPrefs.mockResolvedValue(prefs({ platform: 'mac' }));

  renderSection();

  expect(await shown('application-shortcut')).toBe('⌥Space');
});

// ---------------------------------------------------------------------------
// The recorder.
// ---------------------------------------------------------------------------

// 🔴 (review, Important 1) Named and asserted on its own, not only baked into
// the `record()` helper every other fixture in this section shares. `onkeydown`
// reaches only a FOCUSED element, and clicking a `<button>` does not focus it
// on every platform — macOS WebKit (Tauri's WKWebView) leaves `activeElement`
// as `<body>` after a click. Confirmed red before `Application.svelte` called
// `.focus()`: `document.activeElement` was `<body>`, not the record button.
test('starting the recorder truly focuses the control, so a real keypress reaches it', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await shown('application-shortcut-record');
  const button = screen.getByTestId('application-shortcut-record');
  expect(document.activeElement).not.toBe(button);

  await fireEvent.click(button);

  expect(document.activeElement).toBe(button);
});

test('losing focus by any other route ends the recording, so the sentence does not get stuck', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await record();
  expect(screen.getByTestId('application-shortcut-recording')).toBeTruthy();

  screen.getByTestId('application-shortcut-record').blur();
  await tick();

  expect(screen.queryByTestId('application-shortcut-recording')).toBeNull();
});

test('a recorded combination is sent as one canonical string, and the reply is what gets drawn', async () => {
  appPrefs.mockResolvedValue(prefs());
  setHotkey.mockResolvedValue({ shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } });
  renderSection();
  await record();

  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });

  await waitFor(() => expect(setHotkey).toHaveBeenCalledTimes(1));
  expect(setHotkey).toHaveBeenCalledWith('Ctrl+Alt+Space');
  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+Space'));
  // The recording ended with the press that succeeded.
  expect(screen.queryByTestId('application-shortcut-recording')).toBeNull();
});

test('Escape cancels the recording: nothing is sent, and nothing is refused either', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await record();
  expect(screen.getByTestId('application-shortcut-recording')).toBeTruthy();

  await pressKey({ key: 'Escape', code: 'Escape' });

  expect(setHotkey).not.toHaveBeenCalled();
  // Both directions: the recording is over AND no refusal was put on the
  // screen. A cancel drawn as a refusal tells a person they did something
  // wrong when they only changed their mind.
  expect(screen.queryByTestId('application-shortcut-recording')).toBeNull();
  expect(screen.queryByTestId('application-shortcut-not-usable')).toBeNull();
  expect(at('application-shortcut')).toBe('Alt+Space');
});

test('a key the recorder cannot map is refused in the window own words, and nothing is sent', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await record();

  await pressKey({ key: 'x', code: 'IntlBackslash', ctrlKey: true });

  expect(setHotkey).not.toHaveBeenCalled();
  // 🔴 A refusal the WINDOW makes comes from the catalogue, in the active
  // language — not from the parser, whose sentence for this string asks the
  // reader to report it on GitHub. `Super` because the default fixture's
  // platform is `linux` (review, Minor 5: the modifier's name is platform-aware
  // now, never the platform-neutral "the command key").
  expect(at('application-shortcut-not-usable'))
    .toBe('Цю клавішу не можна використати в скороченні. Скорочення — це літера, цифра, функційна клавіша, стрілка або пробіл, натиснуті разом принаймні з однією з клавіш Ctrl, Alt, Shift чи Super.');
});

test('the not-usable sentence names the platform`s own modifier key', async () => {
  appPrefs.mockResolvedValue(prefs({ platform: 'mac' }));
  renderSection();
  await record();

  await pressKey({ key: 'x', code: 'IntlBackslash', ctrlKey: true });

  expect(at('application-shortcut-not-usable')).toContain('Cmd');
  expect(at('application-shortcut-not-usable')).not.toContain('Super');
});

test('a key pressed with no modifier is refused, and nothing is sent', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await record();

  await pressKey({ key: 'a', code: 'KeyA' });

  expect(setHotkey).not.toHaveBeenCalled();
  expect(screen.getByTestId('application-shortcut-not-usable')).toBeTruthy();
  // Both directions: the very same key WITH a modifier does get sent, so the
  // refusal above is about the missing modifier and not about the key.
  setHotkey.mockResolvedValue({ shortcut: 'Ctrl+A', status: { kind: 'registered' } });
  await pressKey({ key: 'a', code: 'KeyA', ctrlKey: true });
  await waitFor(() => expect(setHotkey).toHaveBeenCalledWith('Ctrl+A'));
});

test('holding a modifier on the way to a combination is ignored, not refused', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await record();

  await pressKey({ key: 'Control', code: 'ControlLeft', ctrlKey: true });

  expect(setHotkey).not.toHaveBeenCalled();
  // 🔴 The state a suite without it cannot read: every press of Ctrl on the way
  // to Ctrl+A would otherwise flash a refusal. The recording is still open.
  expect(screen.queryByTestId('application-shortcut-not-usable')).toBeNull();
  expect(screen.getByTestId('application-shortcut-recording')).toBeTruthy();
});

// ---------------------------------------------------------------------------
// A refused change — the pair. §10: a rejection is a SENTENCE and carries no
// state at all, so what the screen shows afterwards can only come from a fresh
// read.
// ---------------------------------------------------------------------------

test('a refused change shows the sentence and then draws the NEW shortcut when a fresh read reports it', async () => {
  // D-b's persist-failure row: `set_hotkey` rejects with `Error::Prefs` while
  // the new shortcut is registered and the old one is still in `prefs.json`. A
  // section that redrew its pre-call value would state a fact the operating
  // system contradicts.
  const SENTENCE = 'the preferences file could not be written';
  appPrefs.mockResolvedValueOnce(prefs());
  setHotkey.mockRejectedValue(new Error(SENTENCE));
  appPrefs.mockResolvedValueOnce(prefs({
    hotkey: { shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } },
  }));
  renderSection();
  await record();

  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });

  expect(await shown('application-shortcut-error')).toBe(SENTENCE);
  expect(at('application-shortcut-failed')).toBe('Скорочення не змінено. Ось що відповів застосунок:');
  // Strict equality on the dedicated testid, not `toContain` on the page: the
  // new shortcut, 'Ctrl+Alt+Space', contains the old one, 'Alt+Space', as a
  // literal substring, so a page-wide `.not.toContain('Alt+Space')` could never
  // tell the two apart — an implementation that kept redrawing the OLD value
  // would satisfy it exactly as well as this correct one does. `.toBe` on the
  // element the section itself designates for the shortcut is what actually
  // discriminates re-reading `appPrefs()` from redrawing the pre-call value.
  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+Space'));
  await waitFor(() => expect(appPrefs).toHaveBeenCalledTimes(2));
});

test('a refused change draws the OLD shortcut when the fresh read still reports that one', async () => {
  // The other half of the pair, and only the two together tell a re-read from a
  // lucky guess: this one passes under an implementation that never re-reads.
  const SENTENCE = 'HotKey already registered by another application';
  appPrefs.mockResolvedValueOnce(prefs());
  setHotkey.mockRejectedValue(new Error(SENTENCE));
  appPrefs.mockResolvedValueOnce(prefs());
  renderSection();
  await record();

  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });

  expect(await shown('application-shortcut-error')).toBe(SENTENCE);
  await waitFor(() => expect(appPrefs).toHaveBeenCalledTimes(2));
  expect(at('application-shortcut')).toBe('Alt+Space');
});

test('a change that succeeds after one that was refused takes the failure sentence away with it', async () => {
  appPrefs.mockResolvedValue(prefs());
  setHotkey.mockRejectedValueOnce(new Error('refused once'));
  renderSection();
  await record();
  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });
  await waitFor(() => expect(screen.getByTestId('application-shortcut-error')).toBeTruthy());

  setHotkey.mockResolvedValue({ shortcut: 'Ctrl+Alt+A', status: { kind: 'registered' } });
  await record();
  await pressKey({ key: 'a', code: 'KeyA', altKey: true, ctrlKey: true });

  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+A'));
  expect(screen.queryByTestId('application-shortcut-error')).toBeNull();
  expect(screen.queryByTestId('application-shortcut-failed')).toBeNull();
});

// 🔴 (review, Important 2) Two writers share one generation stamp. A rejected
// `setHotkey` starts a `refresh()` (bumping `seq`) that this fixture holds
// open; before it settles, a SECOND recording succeeds and writes `prefs`
// directly. Without also bumping `seq` in that success path, the held-open
// `refresh()` is not superseded — it resolves later with the STALE pre-change
// read and overwrites the change that already landed, stating a shortcut the
// operating system is not actually holding.
test('a change that succeeds must supersede a refresh() still in flight from an earlier rejection', async () => {
  const queue: ReturnType<typeof deferred<AppPrefs>>[] = [];
  appPrefs.mockImplementation(() => {
    const d = deferred<AppPrefs>();
    queue.push(d);
    return d.promise;
  });
  setHotkey.mockRejectedValueOnce(new Error('refused'));
  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  queue[0].resolve(prefs());
  await record();

  // First attempt: refused, which starts a second `appPrefs()` read — held
  // open rather than resolved.
  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });
  await waitFor(() => expect(queue).toHaveLength(2));

  // Before that read settles, a second recording succeeds outright.
  setHotkey.mockResolvedValue({ shortcut: 'Ctrl+Alt+A', status: { kind: 'registered' } });
  await record();
  await pressKey({ key: 'a', code: 'KeyA', altKey: true, ctrlKey: true });
  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+A'));

  // The stale re-read now resolves with the OLD, pre-change state. It must not
  // repaint over the change that already landed.
  queue[1].resolve(prefs());
  await tick();
  await tick();

  expect(at('application-shortcut')).toBe('Ctrl+Alt+A');
});

// 🔴 (final review, D-I2) The record control had no in-flight guard while its
// autostart sibling did, and the reasoning that justified deferring the twin
// does not transfer: two presses of the toggle carry the same value, two
// recorded combinations carry two DIFFERENT ones. `change_hotkey` serialises
// them behind one critical section — `two_hotkey_changes_cannot_interleave` —
// so the operating system and `prefs.json` end up holding whichever went
// through the lock last, while this window paints whichever reply resolved
// last. Those need not be the same one, and the result is a screen naming a
// shortcut the operating system is not holding.
test('the record control is busy while a change is in flight, so a second press sends one call and does not reopen the recorder', async () => {
  appPrefs.mockResolvedValue(prefs());
  const inFlight = deferred<AppPrefs['hotkey']>();
  setHotkey.mockReturnValue(inFlight.promise);
  renderSection();
  const button = () => screen.getByTestId<HTMLButtonElement>('application-shortcut-record');
  await record();
  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });

  expect(setHotkey).toHaveBeenCalledTimes(1);
  expect(button().disabled).toBe(true);

  // The second press, and then a second combination at whatever is listening:
  // the click alone only reopens the recorder, and it is the keypress after it
  // that would send the second call.
  await fireEvent.click(button());
  expect(screen.queryByTestId('application-shortcut-recording')).toBeNull();
  await pressKey({ key: 'a', code: 'KeyA', altKey: true, ctrlKey: true });
  expect(setHotkey).toHaveBeenCalledTimes(1);

  // And the control comes back, so a refusal does not cost a person the only
  // way to change the shortcut.
  inFlight.resolve({ shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } });
  await waitFor(() => expect(button().disabled).toBe(false));
  await record();
  expect(screen.getByTestId('application-shortcut-recording')).toBeTruthy();
});

// 🔴 (final review, D-I1) One stamp for three writers over DISJOINT fields, and
// the two that matter are driven together here for the first time. A refused
// `setHotkey` starts the re-read D-b requires — a rejection carries no
// `HotkeyState`, so a fresh `appPrefs()` is the only honest source for what the
// screen draws next. A field-blind stamp lets an unrelated `setAutostart` that
// succeeds in the meantime discard that whole read, and the state it leaves
// wrong is D-b's persist-failure row: `set_hotkey` rejects with `Error::Prefs`
// while the operating system IS holding the new shortcut, so the window goes on
// drawing the old one beside "the shortcut was not changed".
//
// Both halves are asserted, because either alone is satisfied by a wrong
// implementation: keeping the read whole would draw the pre-toggle autostart,
// and dropping it whole draws the pre-change shortcut.
test('a successful autostart change must not discard the re-read a refused shortcut change started', async () => {
  const queue: ReturnType<typeof deferred<AppPrefs>>[] = [];
  appPrefs.mockImplementation(() => {
    const d = deferred<AppPrefs>();
    queue.push(d);
    return d.promise;
  });
  setHotkey.mockRejectedValue(new Error('the shortcut could not be saved'));
  setAutostart.mockResolvedValue({ kind: 'enabled' });
  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  queue[0].resolve(prefs({ autostart: { kind: 'disabled' } }));
  await record();

  // The change is refused, so the corrective read starts — held open.
  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });
  await waitFor(() => expect(queue).toHaveLength(2));

  // Before it settles, the OTHER writer succeeds. Nothing about autostart says
  // anything about the shortcut.
  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));
  await waitFor(() => expect(at('application-autostart-status'))
    .toBe('Mnema запускається під час входу в систему.'));

  // The held-open read now answers with what the operating system actually
  // holds: the registration succeeded and only the persist failed, so the
  // shortcut is the NEW one. It carries the pre-toggle autostart with it,
  // because it was issued before that press.
  queue[1].resolve(prefs({
    hotkey: { shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'disabled' },
  }));
  await tick();
  await tick();

  expect(at('application-shortcut')).toBe('Ctrl+Alt+Space');
  expect(at('application-autostart-status')).toBe('Mnema запускається під час входу в систему.');
});

// The mirror, so the stamps are not merely two names for one order: a refused
// `setAutostart` starts its own re-read, and a `setHotkey` that succeeds while
// it is in flight must not discard it either. Without a stamp per field this
// direction is the one that passes by accident, since the surviving read would
// happen to carry the right autostart.
test('a successful shortcut change must not discard the re-read a refused autostart change started', async () => {
  const queue: ReturnType<typeof deferred<AppPrefs>>[] = [];
  appPrefs.mockImplementation(() => {
    const d = deferred<AppPrefs>();
    queue.push(d);
    return d.promise;
  });
  setAutostart.mockRejectedValue(new Error('the login item list could not be written'));
  setHotkey.mockResolvedValue({ shortcut: 'Ctrl+Alt+A', status: { kind: 'registered' } });
  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  queue[0].resolve(prefs({ autostart: { kind: 'disabled' } }));
  await waitFor(() => expect(screen.getByTestId('application-autostart-toggle')).toBeTruthy());

  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));
  await waitFor(() => expect(queue).toHaveLength(2));

  await record();
  await pressKey({ key: 'a', code: 'KeyA', altKey: true, ctrlKey: true });
  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+A'));

  // What the operating system says about autostart, which is the only thing a
  // refused `set_autostart` leaves knowable.
  queue[1].resolve(prefs({
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'unknown', reason: 'the login item list could not be read' },
  }));
  await tick();
  await tick();

  expect(at('application-autostart-status')).toBe('Не вдалося дізнатися, чи запускається Mnema під час входу в систему.');
  expect(at('application-shortcut')).toBe('Ctrl+Alt+A');
});

// ---------------------------------------------------------------------------
// Autostart.
// ---------------------------------------------------------------------------

test('the three autostart states get three sentences, no two alike', async () => {
  const sentences: string[] = [];
  const states: AppPrefs['autostart'][] = [
    { kind: 'enabled' },
    { kind: 'disabled' },
    { kind: 'unknown', reason: 'the login item list could not be read' },
  ];
  for (const autostart of states) {
    appPrefs.mockResolvedValue(prefs({ autostart }));
    renderSection();
    sentences.push(await shown('application-autostart-status'));
    cleanup();
  }

  expect(new Set(sentences).size).toBe(3);
  expect(sentences[0]).toBe('Mnema запускається під час входу в систему.');
  expect(sentences[1]).toBe('Mnema не запускається під час входу в систему.');
  expect(sentences[2]).toBe('Не вдалося дізнатися, чи запускається Mnema під час входу в систему.');
});

test('an unreadable autostart shows the reason, and a readable one has none to show', async () => {
  const REASON = 'the login item list could not be read';
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'unknown', reason: REASON } }));
  renderSection();

  expect(await shown('application-autostart-reason')).toContain(REASON);

  cleanup();
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  renderSection();
  await shown('application-autostart-status');
  expect(screen.queryByTestId('application-autostart-reason')).toBeNull();
});

test('pressing the autostart control sends the opposite of the current value, once', async () => {
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  setAutostart.mockResolvedValue({ kind: 'enabled' });
  renderSection();
  await shown('application-autostart-toggle');

  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));

  await waitFor(() => expect(setAutostart).toHaveBeenCalledTimes(1));
  expect(setAutostart).toHaveBeenCalledWith(true);
  await waitFor(() => expect(at('application-autostart-status'))
    .toBe('Mnema запускається під час входу в систему.'));

  // And from there, back the other way — the value sent follows the state on
  // screen rather than a constant.
  setAutostart.mockResolvedValue({ kind: 'disabled' });
  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));
  await waitFor(() => expect(setAutostart).toHaveBeenCalledTimes(2));
  expect(setAutostart).toHaveBeenLastCalledWith(false);
});

test('the autostart state drawn after a press is the reply, not the request', async () => {
  // 🔴 The state a suite without it cannot read. `set_autostart` re-reads the
  // operating system after the change (D-c), so a reply that disagrees with the
  // request is a real state: the enable was asked for, and the machine could
  // not be asked whether it took. A section echoing its own request would show
  // «запускається» over a machine nobody could read.
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  const REASON = 'the login item list could not be read';
  setAutostart.mockResolvedValue({ kind: 'unknown', reason: REASON });
  renderSection();
  await shown('application-autostart-toggle');

  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));

  await waitFor(() => expect(at('application-autostart-status'))
    .toBe('Не вдалося дізнатися, чи запускається Mnema під час входу в систему.'));
  expect(at('application-autostart-reason')).toContain(REASON);
  expect(pageText()).not.toContain('Mnema запускається під час входу в систему.');
});

test('a refused autostart change shows the backend sentence beside the state it could not change', async () => {
  const SENTENCE = 'the login item could not be written';
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  setAutostart.mockRejectedValue(new Error(SENTENCE));
  renderSection();
  await shown('application-autostart-toggle');

  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));

  expect(await shown('application-autostart-error')).toBe(SENTENCE);
  expect(at('application-autostart-failed')).toBe('Налаштування не змінено. Ось що відповів застосунок:');
  // The state is re-read rather than assumed: a rejection carries no state.
  await waitFor(() => expect(appPrefs).toHaveBeenCalledTimes(2));
  expect(at('application-autostart-status')).toBe('Mnema не запускається під час входу в систему.');
});

// (review, Minor 2) The shortcut side already has this fixture
// (`:415-417` above); the autostart side did not, though `autostartError =
// null` runs at the top of `toggleAutostart` for exactly the same reason.
test('a change that succeeds after a refused autostart change takes the failure sentence away with it', async () => {
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  setAutostart.mockRejectedValueOnce(new Error('refused once'));
  renderSection();
  await shown('application-autostart-toggle');
  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));
  await waitFor(() => expect(screen.getByTestId('application-autostart-error')).toBeTruthy());

  setAutostart.mockResolvedValue({ kind: 'enabled' });
  await fireEvent.click(screen.getByTestId('application-autostart-toggle'));

  await waitFor(() => expect(at('application-autostart-status'))
    .toBe('Mnema запускається під час входу в систему.'));
  expect(screen.queryByTestId('application-autostart-error')).toBeNull();
  expect(screen.queryByTestId('application-autostart-failed')).toBeNull();
});

// (review, Minor 3) No in-flight guard meant a double press sent two
// `set_autostart` calls, both carrying the same value — not a wrong state, but
// the OS was asked twice for nothing and the person got no sign the first
// press had already been received.
test('the autostart control is busy while a change is in flight, so a double press sends one call', async () => {
  appPrefs.mockResolvedValue(prefs({ autostart: { kind: 'disabled' } }));
  const inFlight = deferred<{ kind: 'enabled' }>();
  setAutostart.mockReturnValue(inFlight.promise);
  renderSection();
  const toggle = () => screen.getByTestId<HTMLButtonElement>('application-autostart-toggle');
  await shown('application-autostart-toggle');

  await fireEvent.click(toggle());
  expect(toggle().disabled).toBe(true);
  await fireEvent.click(toggle());

  expect(setAutostart).toHaveBeenCalledTimes(1);

  inFlight.resolve({ kind: 'enabled' });
  await waitFor(() => expect(toggle().disabled).toBe(false));
});

// ---------------------------------------------------------------------------
// The version (D-h).
// ---------------------------------------------------------------------------

test('the version is drawn as it is, 0.0.0 included, with no claim about being up to date', async () => {
  appPrefs.mockResolvedValue(prefs({ version: '0.0.0' }));

  renderSection();

  expect(await shown('application-version')).toBe('Версія 0.0.0');
  expect(pageText()).not.toContain('найновіш');
  expect(pageText()).not.toContain('оновлен');
});

test('a released version is drawn as it is too, so nothing here is special-cased', async () => {
  appPrefs.mockResolvedValue(prefs({ version: '1.4.2' }));

  renderSection();

  expect(await shown('application-version')).toBe('Версія 1.4.2');
});

// ---------------------------------------------------------------------------
// The read itself.
// ---------------------------------------------------------------------------

test('a refused read shows the backend sentence and draws nothing it does not know', async () => {
  const SENTENCE = 'the settings window could not reach the application state';
  appPrefs.mockRejectedValue(new Error(SENTENCE));

  renderSection();

  expect(await shown('application-load-failed')).toBe('Не вдалося прочитати налаштування застосунку.');
  expect(at('application-load-error')).toBe(SENTENCE);
  expect(screen.queryByTestId('application-shortcut')).toBeNull();
  expect(screen.queryByTestId('application-autostart-status')).toBeNull();
  expect(screen.queryByTestId('application-version')).toBeNull();
});

test('an older read that settles last does not repaint over the newer one', async () => {
  // Two reads overlap here whenever a refused change lands while the mount's
  // read is still in flight, and they may settle in either order. Without a
  // stamp the older reply repaints the screen with a shortcut taken before the
  // change — and nothing on the screen says so, because both answers are
  // well-formed.
  const queue: ReturnType<typeof deferred<AppPrefs>>[] = [];
  appPrefs.mockImplementation(() => {
    const d = deferred<AppPrefs>();
    queue.push(d);
    return d.promise;
  });
  setHotkey.mockRejectedValue(new Error('refused'));
  renderSection();
  await waitFor(() => expect(queue).toHaveLength(1));
  queue[0].resolve(prefs());
  await record();

  await pressKey({ key: ' ', code: 'Space', altKey: true, ctrlKey: true });
  await waitFor(() => expect(queue).toHaveLength(2));

  // Newer first, older last — the order the IPC is free to choose.
  queue[1].resolve(prefs({ hotkey: { shortcut: 'Ctrl+Alt+Space', status: { kind: 'registered' } } }));
  await waitFor(() => expect(at('application-shortcut')).toBe('Ctrl+Alt+Space'));
  queue[0].resolve(prefs());
  await tick();
  await tick();

  expect(at('application-shortcut')).toBe('Ctrl+Alt+Space');
});

// ---------------------------------------------------------------------------
// The whole window, read as a person reads it. A section that renders the right
// values under the wrong labels satisfies every testid assertion above.
// ---------------------------------------------------------------------------

test('a person who opens Application in the settings window reads the shortcut, the autostart state and the version', async () => {
  appPrefs.mockResolvedValue(prefs({ platform: 'mac', version: '0.0.0' }));
  const { container } = render(Settings);
  const panel = () => container.querySelector('.spane');

  await fireEvent.click(screen.getByTestId('settings-nav-application'));

  await waitFor(() => expect(screen.getByTestId('application-version')).toBeTruthy());
  // Equality over the whole panel, not containment: the heading also sits in
  // the nav, so a `toContain` over the page is satisfied by the nav alone and
  // never notices an empty panel or a value drawn under the wrong label.
  expect(visible(panel())).toBe(
    'Застосунок'
    + ' Скорочення для відкриття пошуку: ⌥Space'
    + ' Це скорочення зареєстровано в системі.'
    + ' Змінити скорочення'
    + ' Запуск під час входу в систему:'
    + ' Mnema не запускається під час входу в систему.'
    + ' Запускати під час входу'
    + ' Версія 0.0.0',
  );
  // (review, Important 1) A `not.toContain` against `settings_section_not_ready`'s
  // old sentence stood here — Task 8 removed that key from the catalogue, so
  // the string can no longer be produced by anything and the assertion could
  // not fail. The equality check above is strictly stronger: it is exact over
  // the whole panel, not a containment claim over one page, so a placeholder
  // sentence appearing anywhere in the panel would already break it.
});

test('the section speaks the window language, both directions', async () => {
  appPrefs.mockResolvedValue(prefs());
  renderSection();
  await shown('application-autostart-status');

  setLocale('en');

  await waitFor(() => expect(at('application-autostart-status'))
    .toBe('Mnema does not start when you sign in.'));
  expect(at('application-shortcut-status')).toBe('This shortcut is registered with the system.');
  expect(at('application-version')).toBe('Version 0.0.0');
  // Gone, not merely joined.
  expect(pageText()).not.toContain('Mnema не запускається під час входу в систему.');
  // D-h in the other language too.
  expect(pageText()).not.toContain('up to date');
});
