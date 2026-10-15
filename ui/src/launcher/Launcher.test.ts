import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { READINESS_FIXTURES } from '../lib/fixtures';
import { roleReady } from './state';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { vi, expect, test, beforeEach } from 'vitest';
import Launcher from './Launcher.svelte';
import { refusedNoCandidates, generated, generatedOther, citationsOnly, emptyCitationsOnly, oneRootTwoFolders, excerptSpanA } from '../lib/fixtures';
import { DRAG_GRAB_WINDOW_MS } from './state';
import { setLocale } from '../i18n';
const HERE = dirname(fileURLToPath(import.meta.url));

const hide = vi.fn();
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ hide }) }));
// The launcher listens for `launcher-cold`; the test fires it by hand.
const cold = vi.hoisted(() => ({ handlers: [] as Array<() => void>, names: [] as string[] }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, cb: () => void) => {
    cold.names.push(name);
    cold.handlers.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireCold = () => cold.handlers.at(-1)!();

const invoke = vi.fn();
// What `provider_status` answers; a test swaps it to drive the cloud.
const okStatus = () => Promise.resolve({ kind: 'ok' });
let providerReply: () => Promise<unknown> = okStatus;
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// Answers each command separately. `model_settings` (the launcher's mount
// seed) always resolves so it never crashes a render; `ask` is what each
// test controls.
//
// 🔴 Ruling Y, and it is not optional: from Task 8b `Cards` mounts `Tree`, which
// calls `list_tree` on mount. Every test that reaches state B goes through it,
// and the bare `Promise.resolve()` this function used to give unknown commands
// made the card read `.roots` off `undefined`.
//
// A launcher test that never clicks a citation never reaches `source_around`, so
// that one stays unmocked on purpose — but M2 (review round 1) measured what
// that actually looks like, and only half of it is loud: vitest reports an
// unhandled `TypeError: Cannot read properties of undefined (reading 'kind')`,
// so the run is not green, yet the card still PAINTS — a settled
// `data-pending="0"` `source-failed` card under a correct header. A future test
// that waits on `card-source` and asserts anything other than the excerpt would
// pass against it. Mock the command rather than trusting the crash.
const READY_INDEX = { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: 'emb', chatModel: 'chat', searchTextArm: true, searchContentArm: false };
// Both roles on OpenRouter with a key and a chosen model each: the cloud may say `ok`.
const SETTINGS_READY = {
  key: { kind: 'present' }, index: READY_INDEX, roles: { embed: 'openRouter', chat: 'openRouter' },
  configured: { openRouter: true, mnemaEmbed: false, mnemaChat: false },
};
function mockBackend(askReply: unknown, opts: { reject?: boolean } = {}) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    if (cmd === 'ask') return opts.reject ? Promise.reject(askReply) : Promise.resolve(askReply);
    return Promise.resolve();
  });
}
const askCalls = () => invoke.mock.calls.filter((c) => c[0] === 'ask');
const layoutCalls = () => invoke.mock.calls.filter((c) => c[0] === 'set_launcher_layout').map((c) => c[1]);
const listTreeCalls = () => invoke.mock.calls.filter((c) => c[0] === 'list_tree');

// Answers each `ask` in turn, so one test can drive two questions with different
// outcomes — the refusal path (ruling I-B) needs a generated answer first and a
// refusal second. Everything else answers as `mockBackend` does.
function mockAsks(...replies: unknown[]) {
  let next = 0;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    // Loud past the end, never a silent repeat (M2). `Cards.test.ts` takes the
    // same policy for the same reason: a test that asks once more than its
    // author intended would get the previous answer back and attribute whatever
    // it then asserts to the wrong cause.
    if (cmd === 'ask') {
      if (next >= replies.length) throw new Error(`mockAsks: no reply for ask #${next + 1}`);
      return Promise.resolve(replies[next++]);
    }
    return Promise.resolve();
  });
}

// The product-level sequences below share this opening: one answered question
// and a folder opened by hand — the state a person is actually in when they type
// the next thing.
//
// 🔴 M1: `expect(listTreeCalls()).toHaveLength(1)` used to live here and does
// not any more. It is a CLAIM belonging to the two "does not refetch" tests, not
// a precondition of the four that share this opening — and while it sat here, a
// keyed-tree mutant killed both "does not shut a folder" tests INSIDE the helper,
// on a count, before either reached its own assertion. That is I-A's shape one
// level up, inside the helper built to fix I-A. What stays is a real
// precondition: the folder actually opened.
async function askAndOpenAFolder() {
  render(Launcher);
  await submit('first question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('first question'));
  await fireEvent.click(await screen.findByTestId('tree-folder-archive'));
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
}

// Default so the retained PR 2 tests (which just render) never hit an unmocked
// command; each ask test overrides with its own reply.
beforeEach(() => { providerReply = okStatus; hide.mockClear(); invoke.mockReset(); cold.handlers.length = 0; cold.names.length = 0; mockBackend(undefined); });

async function submit(value: string) {
  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value } });
  await fireEvent.keyDown(box, { key: 'Enter' });
}

test('a query that refuses shows the F message', async () => {
  mockBackend(refusedNoCandidates);
  render(Launcher);
  await submit('nothing indexed');
  expect(invoke).toHaveBeenCalledWith('ask', { query: 'nothing indexed' });
  await screen.findByRole('status'); // F message appears
  expect(screen.getByRole('status').textContent).toMatch(/found|знайдено/i);
});

// Ruling Z: this test used to make both claims at once. The echo half moved to
// its own test below the moment the bubble stopped being the launcher's: §7
// gives state F no bubble at all, and `Cards` draws nothing there, so asserting
// one here would now be asserting something the design says must not exist.
// The line-clears half is true in every ready state and stays on the refusal.
test('on ready the line clears', async () => {
  mockBackend(refusedNoCandidates); // any successful answer clears the line; a refusal is one
  render(Launcher);
  await submit('echo me');
  await screen.findByRole('status');
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe(''); // line cleared
  expect(screen.queryByTestId('query-echo')).toBeNull(); // and state F shows no bubble at all
});

// D171: the provider failed to answer — the passages show, and the query stays
// in the line so Enter asks again. Both directions: `notAsked` (no chat model)
// is not a failure to retry, and clears the line like every other answer.
test('a provider failure keeps the query in the line; a model never asked clears it', async () => {
  for (const why of [{ kind: 'offline' }, { kind: 'noReply' }, { kind: 'embeddingNoReply' }, { kind: 'failed', reason: 'r' }, { kind: 'localLoading' }]) {
    mockBackend({ ...citationsOnly, why });
    render(Launcher);
    await submit('retry me');
    await screen.findByTestId('citations-banner');
    expect((screen.getByRole('textbox') as HTMLInputElement).value, why.kind).toBe('retry me');
    // The cloud is asked again after the failure, not left on its cached state.
    await waitFor(() => expect(invoke.mock.calls.filter((c) => c[0] === 'provider_status').length, why.kind).toBeGreaterThan(1));
    cleanup();
  }

  mockBackend(citationsOnly); // why: notAsked
  render(Launcher);
  await submit('clear me');
  await screen.findByTestId('citations-banner');
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('');
});

// The echo half of the split. The bubble is drawn by `Answer` inside the centre
// card now (Task 8b), so it exists only where an answer does — state B.
test('the submitted query echoes as a bubble on a generated answer', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('echo me');
  expect(await screen.findByTestId('query-echo')).toBeTruthy();
  expect(screen.getByTestId('query-echo').textContent).toBe('echo me');
});

// 🔴 The mutation proof for deleting the launcher's own `query-echo` div
// (Ruling Z): `Answer` draws one, the launcher drew another, and in state B both
// were on screen. Leave the div in place and this reads 2.
test('the launcher renders exactly one query bubble in state B', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('how much?');
  expect(await screen.findAllByTestId('query-echo')).toHaveLength(1);
});

test('a draft typed while an ask is in flight survives the ready-clear (Codex #3)', async () => {
  // The input stays editable in state D. If a user types a new draft while the
  // first ask is pending, the unconditional `query=''` on ready would wipe it.
  // Clear only when the line still holds the submitted query.
  let resolveAsk!: (v: unknown) => void;
  const pending = new Promise((r) => { resolveAsk = r; });
  // 🔴 C1 widened Ruling Y: this test installs its own implementation instead of
  // `mockBackend`, and it is the only launcher test that SITS in state D — where
  // the tree card now stays up. Without `list_tree` here the tree reads `.roots`
  // off `undefined` and vitest reports an unhandled `TypeError` while every test
  // still passes.
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    if (cmd === 'ask') return pending;
    return Promise.resolve();
  });
  render(Launcher);
  await submit('first question'); // Q1 → in flight
  const box = screen.getByRole('textbox') as HTMLInputElement;
  await fireEvent.input(box, { target: { value: 'second draft' } }); // type Q2 while pending
  resolveAsk(refusedNoCandidates); // Q1's answer arrives
  await screen.findByRole('status'); // ready
  expect(box.value).toBe('second draft'); // the draft was NOT wiped by the clear
  // Ruling Z: the echo assertion is gone, not moved. This test's claim is
  // `box.value`, `findByRole('status')` above is already its readiness marker,
  // and state F draws no bubble to assert on any more.
});

test('a rejected ask is visible and logged, not swallowed', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockBackend('the index is not open', { reject: true }); // what with_index → IndexNotOpen becomes on the wire
  render(Launcher);
  await submit('q');
  await screen.findByRole('alert');
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
  expect(err).toHaveBeenCalled(); // logged, not a silent reset
  err.mockRestore();
});

// A failed ask is an error, not an answer: a hot launcher keeps the answer it
// already showed and says what failed in the search line. A refusal is an
// answer ("nothing found") and still replaces it.
function mockAsksThenReject(first: unknown) {
  let n = 0;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    if (cmd === 'ask') return n++ === 0 ? Promise.resolve(first) : Promise.reject('offline');
    return Promise.resolve();
  });
}

test('a failed ask in the hot state brings the previous answer back and shows the error in the line', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockAsksThenReject(generated);
  render(Launcher);
  await submit('first');
  const before = (await screen.findByTestId('card-centre')).textContent;
  await submit('second');
  await screen.findByRole('alert');
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('card-source')).toBeTruthy();
  err.mockRestore();
});

test('two failed asks in a row keep the answer and its echo', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockAsksThenReject(generated);
  render(Launcher);
  await submit('first');
  const before = (await screen.findByTestId('card-centre')).textContent;
  await submit('second');
  await screen.findByRole('alert');
  await submit('third');
  await waitFor(() => expect(askCalls()).toHaveLength(3));
  await screen.findByRole('alert');
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('query-echo').textContent).toBe('first');
  err.mockRestore();
});

test('a failed ask in the cold state stays an error with no answer cards', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockBackend('offline', { reject: true });
  render(Launcher);
  await submit('q');
  await screen.findByRole('alert');
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
  err.mockRestore();
});

test('a failed ask does not restart the idle clock', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockAsksThenReject(generated);
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await submit('second');
  await screen.findByRole('alert');
  expect(invoke.mock.calls.filter((c) => c[0] === 'launcher_answered')).toHaveLength(1);
  err.mockRestore();
});

test('a generated answer renders the centre card, not a refusal', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-centre');
  // Not a refusal. The source card carries a `status` of its own, so the role
  // alone says nothing: the refusal is the line's message.
  expect(screen.queryAllByRole('status').filter((el) => /found|знайдено/i.test(el.textContent ?? ''))).toHaveLength(0);
});

// 🔴 C1, and the only place it can be seen: `Cards.test.ts` drives `Cards` by
// hand and can build a transition the product never performs, while these tests
// drive the real `runSearch`. `Cards` gates its cards on the launcher's state
// and `runSearch` sets `inFlight` before EVERY ask (`runSearch` in `Launcher.svelte`), so a
// tree drawn only for a generated answer is torn down and refetched in the
// middle of every question — the outcome Ruling AC forbids, reached with no
// `{#key}` anywhere. Both are anchored on the echo, which only a RESOLVED ask
// can write (`runSearch` in `Launcher.svelte`).
//
// 🔴 I-A: two tests, not one with two assertions. Against the only mutant that
// exists for them — the pre-fix gate — a single test fails on the `list_tree`
// count and the folder assertion is never reached, which is the exact shape
// `Cards.test.ts` split apart one level down.
test('a second question does not refetch the tree', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();
  expect(listTreeCalls()).toHaveLength(1); // the precondition of the claim below, and only of it

  await submit('second question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('second question'));

  expect(listTreeCalls()).toHaveLength(1);
});

test('a second question does not shut a hand-opened folder', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('second question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('second question'));

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

// 🔴 Ruling I-B, the refusal path. §7 calls only state A «лише рядок пошуку»;
// row F says «тихе повідомлення», which is no more "only the line" than row D is — and the reason the tree survives D (its content is the INDEX, not
// the answer) never depended on which answer came back. Measured before the
// widening: a question that finds nothing destroyed the whole tree card and
// every folder the person had opened. Anchored on the refusal text, which is
// unique in the catalogue (`catalog.ts:55`) and which only a resolved ask writes.
test('a refusal does not refetch the tree', async () => {
  mockAsks(generated, refusedNoCandidates);
  await askAndOpenAFolder();
  expect(listTreeCalls()).toHaveLength(1);

  await submit('nothing indexed');
  await screen.findByText(/Nothing was found/i);

  expect(screen.getByTestId('card-tree')).toBeTruthy();
  expect(screen.queryByTestId('card-centre')).toBeNull(); // the answer card really is gone
  expect(listTreeCalls()).toHaveLength(1);
});

test('a refusal does not shut a hand-opened folder', async () => {
  mockAsks(generated, refusedNoCandidates);
  await askAndOpenAFolder();

  await submit('nothing indexed');
  await screen.findByText(/Nothing was found/i);

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

test('a second submit while in flight is ignored — one ask at a time', async () => {
  mockBackend(new Promise(() => {})); // ask never resolves (Promise.resolve of a pending promise stays pending) → in flight
  render(Launcher);
  await submit('first');
  await submit('second');
  expect(askCalls()).toHaveLength(1); // the second Enter did not start a second ask
});

// 🔴 M3: the whole tree gate rests on ONE negative — every other state keeps the
// card — and until now that negative lived only in `Cards.test.ts`, driven by
// hand. C1 was invisible at exactly that level for 143 green tests. This puts
// the negative where the positives already are: the first screen a person ever
// sees, through the real component tree.
test('a freshly mounted launcher shows no cards at all (state A)', () => {
  render(Launcher);
  expect(screen.queryByTestId('card-tree')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
});

// --- ruling I-C: `error` is taken whole, and both halves are defended --------
//
// The gate keeps the tree for `error`, and `error` carries two reasons. The
// `Cards`-level test pins `reason: 'tooLong'`; these pin the transition the ruling
// was actually argued from — a person with three cards on screen mistyping an
// Enter — which is the half that makes "do not narrow the gate by reason"
// falsifiable. Anchored on the guard message, which only a completed validation
// writes (`SearchLine.svelte:46`).
test('a too-long Enter from state B keeps the tree (blank Enter is inert; see the empty-line test)', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('x'.repeat(2049));
  await screen.findByRole('alert');

  expect(screen.getByTestId('card-tree')).toBeTruthy();
});

test('a too-long Enter from state B does not shut a hand-opened folder (blank Enter is inert; see the empty-line test)', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('x'.repeat(2049));
  await screen.findByRole('alert');

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

test('a too-long Enter from state B keeps the answer and source cards (blank Enter is inert; see the empty-line test)', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();
  const before = screen.getByTestId('card-centre').textContent;

  await submit('x'.repeat(2049));
  await screen.findByRole('alert');

  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('card-source')).toBeTruthy();
  expect(screen.getByTestId('query-echo').textContent).toBe('first question');
});

test('a too-long Enter from state B keeps the answer and source cards', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();
  const before = screen.getByTestId('card-centre').textContent;

  await submit('x'.repeat(2049));
  await screen.findByRole('alert');

  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('card-source')).toBeTruthy();
  expect(screen.getByTestId('query-echo').textContent).toBe('first question');
});

test('the launcher renders a search input', () => {
  render(Launcher);
  expect(screen.getByRole('textbox')).toBeTruthy();
});

test('Escape hides the launcher', async () => {
  render(Launcher);
  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(hide).toHaveBeenCalledOnce();
});

test('the launcher gaining focus puts the cursor in the search line', async () => {
  render(Launcher);
  const input = screen.getByRole('textbox');
  // Start from somewhere else, so a focus that was already there cannot pass.
  screen.getByTestId('pin').focus();
  expect(document.activeElement).not.toBe(input);
  await fireEvent.focus(window);
  expect(document.activeElement).toBe(input);
});

test('Cmd+, and Ctrl+, open the settings window; a bare comma does not', async () => {
  render(Launcher);
  const openCalls = () => invoke.mock.calls.filter((c) => c[0] === 'open_settings');
  await fireEvent.keyDown(window, { key: ',' });
  expect(openCalls()).toHaveLength(0);
  await fireEvent.keyDown(window, { key: ',', metaKey: true });
  expect(openCalls()).toHaveLength(1);
  await fireEvent.keyDown(window, { key: ',', ctrlKey: true });
  expect(openCalls()).toHaveLength(2);
});

test('click-outside (blur) hides the launcher when it is not pinned', async () => {
  render(Launcher);
  await fireEvent.blur(window);
  expect(hide).toHaveBeenCalledOnce();
});

test('the pin button says whether it is pressed, in both states', async () => {
  // `aria-pressed` is the only way a screen-reader user learns the launcher is
  // pinned — the class beside it paints a colour and says nothing. It was
  // asserted nowhere: the blur test below clicks the button and then watches
  // `hide`, which is true of a button carrying no state at all.
  render(Launcher);
  const pin = screen.getByTestId('pin');
  expect(pin.getAttribute('aria-pressed')).toBe('false');
  await fireEvent.click(pin);
  expect(pin.getAttribute('aria-pressed')).toBe('true');
});

test('a pinned launcher ignores click-outside (blur) — the pin disables it', async () => {
  render(Launcher);
  await fireEvent.click(screen.getByRole('button', { name: /pin|пін|📌/i }));
  hide.mockClear();
  await fireEvent.blur(window);
  expect(hide).not.toHaveBeenCalled();
});

// D155/L2: mutter's move grab (what Tauri's start_dragging does on X11) takes
// keyboard focus for the whole drag and hands it back at the end — the
// webview sees that as a `blur` a few ms after the press on the handle. That
// blur is the drag, not a dismissal.
test('a blur right after a press on the drag handle is the drag, not a dismissal', () => {
  vi.useFakeTimers({ toFake: ['Date'] });
  try {
    mockBackend(generated);
    const { container } = render(Launcher);
    const handle = container.querySelector('.sb-tools')!; // inside .searchbar, not clickable
    fireEvent.pointerDown(handle, { button: 0 });
    vi.advanceTimersByTime(600);
    fireEvent.blur(window);
    expect(hide).not.toHaveBeenCalled();
    // Later, the same blur is a dismissal again — nothing stays armed.
    vi.advanceTimersByTime(DRAG_GRAB_WINDOW_MS);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(1);
  } finally { vi.useRealTimers(); }
});

test('a release after the press disarms the drag window: a click on the handle, then a blur, hides', () => {
  vi.useFakeTimers({ toFake: ['Date'] });
  try {
    mockBackend(generated);
    const { container } = render(Launcher);
    const handle = container.querySelector('.sb-tools')!;
    fireEvent.pointerDown(handle);
    vi.advanceTimersByTime(50);
    fireEvent.pointerUp(handle);
    vi.advanceTimersByTime(50);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(1);
  } finally { vi.useRealTimers(); }
});

// D155 deferred minor: Tauri's own drag script (`drag.js`) starts a move only
// for `button === 0`, so a right- or middle-button press on the handle never
// becomes a drag — and must not arm the guard, or the next dismissal within a
// second is swallowed. `fireEvent.pointerDown` drops `button` (jsdom has no
// `PointerEvent`), so this one is built from `MouseEvent` by hand.
test('a right-button press on the handle does not arm the drag window', () => {
  vi.useFakeTimers({ toFake: ['Date'] });
  try {
    mockBackend(generated);
    const { container } = render(Launcher);
    const handle = container.querySelector('.sb-tools')!;
    handle.dispatchEvent(new MouseEvent('pointerdown', { button: 2, bubbles: true }));
    vi.advanceTimersByTime(50);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(1);
  } finally { vi.useRealTimers(); }
});

// D155 deferred minor: on X11 the drag ends with no release reaching the
// webview, so an arming can outlive its drag. The next press anywhere that
// is not the handle must disarm it on its own, with no release to help — a
// press held in the input, then a blur, is a dismissal.
test('a press off the handle disarms an earlier arming even with no release', () => {
  vi.useFakeTimers({ toFake: ['Date'] });
  try {
    mockBackend(generated);
    const { container } = render(Launcher);
    fireEvent.pointerDown(container.querySelector('.sb-tools')!, { button: 0 });
    vi.advanceTimersByTime(50);
    fireEvent.pointerDown(screen.getByRole('textbox'), { button: 0 });
    vi.advanceTimersByTime(50);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(1);
  } finally { vi.useRealTimers(); }
});

test('a press on the input or the pin does not arm the drag window', () => {
  vi.useFakeTimers({ toFake: ['Date'] });
  try {
    mockBackend(generated);
    render(Launcher);
    fireEvent.pointerDown(screen.getByRole('textbox'), { button: 0 });
    vi.advanceTimersByTime(50);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(1);
    fireEvent.pointerDown(screen.getByTestId('pin'), { button: 0 });
    vi.advanceTimersByTime(50);
    fireEvent.blur(window);
    expect(hide).toHaveBeenCalledTimes(2);
  } finally { vi.useRealTimers(); }
});

test('the search panel is the drag handle and nothing else is', async () => {
  // `Cards` renders a different component per backend state (review finding
  // 7): a `data-tauri-drag-region` added to, say, the refusal card would pass
  // this guard if only the idle state were checked. Submit a question in each
  // of the two states the fixtures already imported at the top of the file
  // cover, and wait for that state's own card before checking.
  for (const reply of [generated, refusedNoCandidates]) {
    mockBackend(reply);
    const { container } = render(Launcher);
    await submit('drag region check');
    if (reply === generated) {
      await screen.findByTestId('card-centre');
    } else {
      await screen.findByRole('status');
    }
    // "deep": any click inside the panel drags, except on the input and the toolbar
    // buttons, which Tauri's own drag script excludes by tag (D155).
    const handles = container.querySelectorAll('[data-tauri-drag-region]');
    expect(Array.from(handles).map((el) => el.className)).toEqual(['searchbar']);
    expect(handles[0].getAttribute('data-tauri-drag-region')).toBe('deep');
    cleanup();
  }
  // The attribute is inert without the permission — guard both in one place.
  const capability = JSON.parse(
    readFileSync(join(HERE, '../../../src-tauri/capabilities/launcher.json'), 'utf8'),
  ) as { windows: string[]; permissions: string[] };
  expect(capability.windows).toContain('launcher');
  expect(capability.permissions).toContain('core:window:allow-start-dragging');
  expect(capability.permissions).not.toContain('core:window:allow-internal-toggle-maximize');
});

test('the handle offset matches the stylesheet', () => {
  // `launcher_position::HANDLE_CENTRE` is written as seven literals in a fixed
  // shape; each one is a declaration in launcher.css. Read both sides and
  // compare numbers, so a change to either without the other goes red — this
  // is the only place the Rust constant and the stylesheet meet (review P2-4).
  // The window is as wide as the panels on screen, so the stylesheet states the
  // tracks once per `data-cols` and every one of them must add up to
  // `launcher_layout::width` for that layout (sum check below).
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  const rust = readFileSync(join(HERE, '../../../src-tauri/src/launcher_position.rs'), 'utf8');
  const num = (re: RegExp, text: string, what: string) => {
    const m = text.match(re);
    if (!m) throw new Error(`${what} not found`);
    return Number(m[1]);
  };
  const panels = css.match(/main\.panels\s*\{[^}]*\}/)![0];
  const searchbar = css.match(/\.searchbar\s*\{[^}]*\}/)![0];
  // The first row is as tall as a toolbar button was when the pin lived in it.
  const pin = css.match(/\.sb-row\s*\{[^}]*\}/)![0];
  // The tracks of one layout, as the stylesheet declares them.
  const tracks = (cols: string) => {
    const block = css.match(new RegExp(`main\\.panels\\[data-cols="${cols}"\\]\\s*\\{[^}]*\\}`));
    if (!block) throw new Error(`no block for data-cols="${cols}"`);
    const value = block[0].match(/grid-template-columns:\s*([^;]+);/);
    if (!value) throw new Error(`no grid-template-columns for data-cols="${cols}"`);
    return value[1].trim().split(/\s+(?![^(]*\))/).map((t) => {
      const px = t.match(/^(?:minmax\(0, )?(\d+)px\)?$/);
      if (!px) throw new Error(`unreadable track ${t} in data-cols="${cols}"`);
      return Number(px[1]);
    });
  };
  const fromCss = {
    padY: num(/padding:\s*(\d+)px \d+px;/, panels, 'main.panels padding-y'),
    padX: num(/padding:\s*\d+px (\d+)px;/, panels, 'main.panels padding-x'),
    gap: num(/gap:\s*(\d+)px;/, panels, 'gap'),
    barPadTop: num(/padding:\s*(\d+)px/, searchbar, '.searchbar padding-top'),
    pinH: num(/min-height:\s*(\d+)px/, pin, '.sb-row min-height'),
  };
  // (1) The widths come from the `lsr` block, not from `main.panels`.
  const [col1, col2, col3] = tracks('lsr');
  const shape = /HANDLE_CENTRE: \(f64, f64\) = \(([\d.]+) \/ 2\.0, ([\d.]+) \+ ([\d.]+) \+ ([\d.]+) \/ 2\.0\);/;
  const m = rust.match(shape);
  if (!m) throw new Error('HANDLE_CENTRE is not written in the guarded shape');
  const fromRust = m.slice(1, 5).map(Number);
  expect(fromRust).toEqual([col2, fromCss.padY, fromCss.barPadTop, fromCss.pinH]);
  // And the numbers are what the spec says today, so a wrong regex that
  // captured the wrong declaration cannot pass by coincidence.
  expect(fromRust).toEqual([470, 0, 11, 26]);
  const conf = JSON.parse(readFileSync(join(HERE, '../../../src-tauri/tauri.conf.json'), 'utf8')) as {
    app: { windows: Array<{ label: string; width: number; resizable?: boolean }> };
  };
  const launcher = conf.app.windows.find((w) => w.label === 'launcher')!;
  expect(launcher.resizable).toBe(false);
  // `launcher_layout::{SEARCH_WIDTH, LEFT_SPAN, RIGHT_SPAN}` restate the same
  // columns for the width/offset math — read the Rust file with a regex, as
  // `launcher_position.rs` already is.
  const layout = readFileSync(join(HERE, '../../../src-tauri/src/launcher_layout.rs'), 'utf8');
  const oneConst = (name: string) => {
    const re = new RegExp(`pub const ${name}: f64 = ([\\d.]+);`);
    const mm = layout.match(re);
    if (!mm) throw new Error(`${name} is not written in the guarded shape`);
    return Number(mm[1]);
  };
  const spanConst = (name: string) => {
    const re = new RegExp(`pub const ${name}: f64 = ([\\d.]+) \\+ ([\\d.]+);`);
    const mm = layout.match(re);
    if (!mm) throw new Error(`${name} is not written in the guarded shape`);
    return Number(mm[1]) + Number(mm[2]);
  };
  const SEARCH_WIDTH = oneConst('SEARCH_WIDTH');
  const LEFT_SPAN = spanConst('LEFT_SPAN');
  const RIGHT_SPAN = spanConst('RIGHT_SPAN');
  expect(SEARCH_WIDTH).toBe(col2);
  expect(LEFT_SPAN).toBe(col1 + fromCss.gap);
  expect(RIGHT_SPAN).toBe(col3 + fromCss.gap);
  // (2) EVERY layout: its tracks and gaps add up to the width Rust gives the
  // window for it, and the grid starts at the left edge, so `search_offset` stays exact.
  const layouts: Array<[string, boolean, boolean]> = [
    ['s', false, false], ['ls', true, false], ['sr', false, true], ['lsr', true, true],
  ];
  for (const [cols, l, r] of layouts) {
    const t = tracks(cols);
    const width = SEARCH_WIDTH + (l ? LEFT_SPAN : 0) + (r ? RIGHT_SPAN : 0);
    expect(t.length, `tracks in ${cols}`).toBe(cols.length);
    expect(2 * fromCss.padX + t.reduce((a, b) => a + b, 0) + (t.length - 1) * fromCss.gap, `width of ${cols}`).toBe(width);
  }
  // (3) The window starts as wide as the cold layout.
  expect(launcher.width).toBe(SEARCH_WIDTH);
  // (4) No narrow-window rule applies at any width the launcher can take: a
  // `max-width` breakpoint at or past the narrowest window swaps the tracks and
  // padding under the constants above. It happened (2026-09-25): the window
  // shrank to 936 under a 959px breakpoint. Zero rules is fine.
  const breakpoints = [...css.matchAll(/@media \(max-width: (\d+)px\)/g)].map((b) => Number(b[1]));
  for (const bp of breakpoints) expect(bp, `breakpoint ${bp}px`).toBeLessThan(SEARCH_WIDTH);
});

test('the grid starts at the window edge and follows data-cols alone', () => {
  // The new window frame shows the new `data-cols` laid out at the old width
  // for one frame. A grid that starts at the left edge is right in that frame;
  // a centred one jumped, and viewport-based margins landed 290 off. So: no
  // centring, no margin, no width media query to place the grid.
  const css = readFileSync(join(HERE, '../styles/launcher.css'), 'utf8');
  const panels = css.match(/main\.panels\s*\{[^}]*\}/)![0];
  expect(panels).not.toMatch(/justify-content:\s*center/);
  expect(panels).toMatch(/justify-content:\s*start/);
  expect(css).not.toMatch(/main\.panels[^{]*\{[^}]*margin-left/);
  expect(css).not.toMatch(/@media[^{]*width/);
  expect(css).not.toMatch(/minmax\(0, 470px\)/);
});

// --- cold and hot ------------------------------------------------------------

const OFF = { left: false, right: false };
const ON = { left: true, right: true };

test('a cold launcher is the search column alone and asks for the narrow window', async () => {
  render(Launcher);
  await waitFor(() => expect(layoutCalls().length).toBeGreaterThan(0));
  for (const id of ['card-tree', 'card-source', 'card-source-empty', 'card-results-empty']) {
    expect(screen.queryByTestId(id), id).toBeNull();
  }
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
  expect(layoutCalls().every((c) => JSON.stringify(c) === JSON.stringify(OFF))).toBe(true);
});

test('the first answer warms the launcher: both panels, the first card open, the wide window', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-source');
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(false);
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('lsr');
  expect(invoke.mock.calls.filter((c) => c[0] === 'source_around')[0][1]).toMatchObject({ chunkId: 42 });
  expect(screen.getByTestId('source-header').textContent).toMatch(/^notes\/a\.md/);
  await waitFor(() => expect(layoutCalls().at(-1)).toEqual(ON));
});

test('the layout is sent once per change, not once per render', async () => {
  mockAsks(generated, citationsOnly);
  render(Launcher);
  await submit('one');
  await screen.findByTestId('card-source');
  const wide = () => layoutCalls().filter((c) => JSON.stringify(c) === JSON.stringify(ON));
  await waitFor(() => expect(wide()).toHaveLength(1));
  await submit('two');
  await waitFor(() => expect(screen.getByTestId('card-centre').getAttribute('aria-label')).toMatch(/passages|уривки|фрагмент/i));
  expect(wide()).toHaveLength(1);
});

test('a refusal from the cold state leaves it cold: no source, no empty panels, the tree not drawn', async () => {
  mockBackend(refusedNoCandidates);
  render(Launcher);
  await submit('nothing');
  await screen.findByRole('status');
  for (const id of ['card-source', 'card-source-empty', 'card-results-empty']) {
    expect(screen.queryByTestId(id), id).toBeNull();
  }
  // Mounted (C1: mounting follows the state) but not drawn.
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(true);
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
  expect(layoutCalls().every((c) => JSON.stringify(c) === JSON.stringify(OFF))).toBe(true);
});

test('an answer with no passages does not warm a cold launcher', async () => {
  mockBackend(emptyCitationsOnly);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-centre');
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(true);
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
});

test('launcher-cold forgets the answer and the panels but keeps the text in the line', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-source');
  await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'a draft' } });

  fireCold();

  await waitFor(() => expect(screen.queryByTestId('card-source')).toBeNull());
  for (const id of ['card-tree', 'card-centre', 'card-source-empty', 'card-results-empty']) {
    expect(screen.queryByTestId(id), id).toBeNull();
  }
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('a draft');
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
  await waitFor(() => expect(layoutCalls().at(-1)).toEqual(OFF));
});

test('a cold launcher warms again on the next answer, with both panels on', async () => {
  mockAsks(generated, generated);
  render(Launcher);
  await submit('one');
  await screen.findByTestId('card-source');
  fireCold();
  await waitFor(() => expect(screen.queryByTestId('card-tree')).toBeNull());
  await submit('two');
  await screen.findByTestId('card-source');
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('lsr');
});

test('a failed ask in the hot state keeps it hot', async () => {
  mockAsks(generated);
  render(Launcher);
  await submit('one');
  await screen.findByTestId('card-source');
  mockBackend(new Error('boom'), { reject: true });
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  await submit('two');
  await screen.findByRole('alert');
  err.mockRestore();
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('lsr');
  expect(screen.getByTestId('card-tree')).toBeTruthy();
  // The previous answer is back, so neither side shows its empty placeholder.
  expect(screen.getByTestId('card-centre')).toBeTruthy();
  expect(screen.queryByTestId('card-results-empty')).toBeNull();
  expect(screen.queryByTestId('card-source-empty')).toBeNull();
});

test('the launcher listens for the event Rust emits', async () => {
  render(Launcher);
  await waitFor(() => expect(cold.names).toContain('launcher-cold'));
});

const answeredCalls = () => invoke.mock.calls.filter((c) => c[0] === 'launcher_answered');

test('an applied answer restarts the idle clock exactly once', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await waitFor(() => expect(screen.getByTestId('card-centre')).toBeTruthy());
  expect(answeredCalls()).toHaveLength(1);
});

test('an answer dropped after launcher-cold does not restart the idle clock', async () => {
  let resolveAsk!: (v: unknown) => void;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'ask') return new Promise((r) => { resolveAsk = r; });
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    return Promise.resolve();
  });
  render(Launcher);
  await submit('slow');
  await waitFor(() => expect(askCalls()).toHaveLength(1));
  fireCold();
  resolveAsk(generated);
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  expect(answeredCalls()).toHaveLength(0);
});

// An ask still on the wire when the launcher goes cold must not bring the
// forgotten answer back, and must not hold the one-ask-at-a-time guard.
test('an ask in flight when launcher-cold arrives is dropped when it resolves', async () => {
  let resolveAsk!: (v: unknown) => void;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'ask') return new Promise((r) => { resolveAsk = r; });
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    return Promise.resolve();
  });
  render(Launcher);
  await submit('slow');
  await waitFor(() => expect(askCalls()).toHaveLength(1));
  fireCold();
  resolveAsk(generated);
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  await waitFor(() => expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s'));
  for (const id of ['card-centre', 'card-source', 'card-tree']) expect(screen.queryByTestId(id), id).toBeNull();
  expect(layoutCalls().some((c) => JSON.stringify(c) === JSON.stringify(ON))).toBe(false);
  // The guard is open again: a new question goes out.
  await submit('fresh');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
});

test('an ask rejected after launcher-cold leaves the cold idle state alone', async () => {
  let rejectAsk!: (v: unknown) => void;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'ask') return new Promise((_, r) => { rejectAsk = r; });
    if (cmd === 'model_settings') return Promise.resolve(SETTINGS_READY);
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve([]);
    return Promise.resolve();
  });
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  render(Launcher);
  await submit('slow');
  await waitFor(() => expect(askCalls()).toHaveLength(1));
  fireCold();
  rejectAsk(new Error('late'));
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(err).not.toHaveBeenCalledWith('ask failed', expect.anything());
  err.mockRestore();
});

// --- the toolbar -------------------------------------------------------------

const toggle = (side: 'left' | 'right') => screen.getByTestId(`toggle-${side}`) as HTMLButtonElement;

test('the pin is in the toolbar, not in the first row', () => {
  const { container } = render(Launcher);
  expect(container.querySelector('.sb-row')).not.toBeNull(); // positive control
  expect(container.querySelector('.sb-row [data-testid="pin"]')).toBeNull();
  expect(container.querySelector('.toolbar [data-testid="pin"]')).not.toBeNull();
});

test('a cold launcher: both toggles are disabled and not pressed', () => {
  render(Launcher);
  for (const side of ['left', 'right'] as const) {
    expect(toggle(side).disabled).toBe(true);
    expect(toggle(side).getAttribute('aria-pressed')).toBe('false');
  }
});

test('turning the left panel off in the hot state tells Rust, and the next answer leaves it off', async () => {
  mockAsks(generated, generated);
  render(Launcher);
  await submit('one');
  await screen.findByTestId('card-source');
  expect(toggle('left').disabled).toBe(false);
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');

  await fireEvent.click(toggle('left'));
  await waitFor(() => expect(layoutCalls().at(-1)).toEqual({ left: false, right: true }));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('false');

  await submit('second question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('second question'));
  expect(toggle('left').getAttribute('aria-pressed')).toBe('false');
  expect(layoutCalls().at(-1)).toEqual({ left: false, right: true });
});

test('the left panel off and on again keeps the tree: hidden, not unmounted (Ruling C1)', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();
  expect(listTreeCalls()).toHaveLength(1);

  await fireEvent.click(toggle('left'));
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(true);
  await fireEvent.click(toggle('left'));
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(false);

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(listTreeCalls()).toHaveLength(1);
});

test('provider_status is asked again when the window gains focus', async () => {
  render(Launcher);
  const count = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd).length;
  await waitFor(() => expect(count('provider_status')).toBe(1));
  await fireEvent.focus(window);
  await waitFor(() => expect(count('provider_status')).toBe(2));
});

const cloud = () => screen.getByTestId('provider-cloud');

test('the cloud shows what provider_status said at mount, and the next focus replaces it', async () => {
  providerReply = () => Promise.resolve({ kind: 'notConfigured', missing: 'key' });
  render(Launcher);
  await waitFor(() => expect(cloud().getAttribute('data-status')).toBe('notConfigured'));
  const before = cloud().getAttribute('title');
  expect(before).toBeTruthy();

  providerReply = okStatus;
  await fireEvent.focus(window);
  await waitFor(() => expect(cloud().getAttribute('data-status')).toBe('ok'));
  expect(cloud().getAttribute('title')).not.toBe(before);
});

test('a slow earlier provider probe cannot overwrite a newer one', async () => {
  let slow!: (v: unknown) => void;
  providerReply = () => new Promise((r) => { slow = r; }); // the mount probe, held open
  render(Launcher);
  await waitFor(() => expect(invoke.mock.calls.filter((c) => c[0] === 'provider_status')).toHaveLength(1));

  providerReply = okStatus;
  await fireEvent.focus(window);
  await waitFor(() => expect(cloud().getAttribute('data-status')).toBe('ok'));

  slow({ kind: 'unreachable', reason: 'late' });
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
  expect(cloud().getAttribute('data-status')).toBe('ok');
});

test('turning the right panel off in the hot state tells Rust', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-source');
  await fireEvent.click(toggle('right'));
  await waitFor(() => expect(layoutCalls().at(-1)).toEqual({ left: true, right: false }));
  expect(toggle('right').getAttribute('aria-pressed')).toBe('false');
  expect(toggle('left').getAttribute('aria-pressed')).toBe('true');
});

// The Toolbar takes the provider from the chat role in `model_settings`, read with the status.
test('under Mnema the status button names Mnema, under OpenRouter it does not', async () => {
  for (const [choice, has, hasNot] of [['mnema', 'Mnema', 'OpenRouter'], ['openRouter', 'OpenRouter', 'Mnema']]) {
    mockBackend(undefined);
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation((cmd: string, ...a: unknown[]) =>
      cmd === 'model_settings' ? Promise.resolve({ ...SETTINGS_READY, roles: { embed: choice, chat: choice }, configured: { openRouter: true, mnemaEmbed: true, mnemaChat: true } }) : base(cmd, ...a));
    render(Launcher);
    const b = await screen.findByTestId('provider-cloud');
    expect(b.getAttribute('aria-label'), choice).toContain(has);
    expect(b.getAttribute('aria-label'), choice).not.toContain(hasNot);
    cleanup();
  }
});

// Owner, live run 2026-10-08: a second Enter on the emptied line put a hint
// under a visible answer. Owner ruling: no hint at all; Enter on an empty line
// does nothing in any state.
test('Enter on an empty line does nothing: nothing on screen, or an answer on screen', async () => {
  mockBackend(generated);
  render(Launcher);
  const box = screen.getByRole('textbox') as HTMLInputElement;
  await fireEvent.keyDown(box, { key: 'Enter' });
  await submit('   ');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(askCalls()).toHaveLength(0);
  await submit('how much?');
  await screen.findByTestId('query-echo');
  await waitFor(() => expect(box.value).toBe(''));
  await fireEvent.keyDown(box, { key: 'Enter' });
  await fireEvent.keyDown(box, { key: 'Enter', repeat: true });
  expect(screen.queryByRole('alert')).toBeNull();
  expect(askCalls()).toHaveLength(1);
  expect(screen.getByTestId('query-echo')).toBeTruthy();
});

// Review of 36436d7, Minor 2: a "too long" message belongs to the line that was
// too long. Once the line is emptied it is stale, and a blank Enter after it
// changes nothing further.
test('the too-long message goes when the line is emptied', async () => {
  mockBackend(generated);
  render(Launcher);
  const box = screen.getByRole('textbox') as HTMLInputElement;
  await submit('x'.repeat(2049));
  expect((await screen.findByRole('alert')).textContent).toContain('2048');
  // Still too long: the message stays (the other direction).
  await fireEvent.keyDown(box, { key: 'Enter' });
  expect(screen.getByRole('alert')).toBeTruthy();
  await fireEvent.input(box, { target: { value: '' } });
  await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  await fireEvent.keyDown(box, { key: 'Enter' });
  expect(screen.queryByRole('alert')).toBeNull();
  expect(askCalls()).toHaveLength(0);
});

// Opening a question from Recents (Task 4). `list_history` answers `entries` and
// a delete takes the row out of it; `history_entry` answers `opened`, or rejects.
// `asks` answers each `ask` in turn and is loud past its end, as `mockAsks` is.
const OLD = { id: 7, query: 'old q', askedAt: 1_700_000_000 };
function mockRecents(opts: { asks?: unknown[]; ask?: Promise<unknown>; opened?: unknown; openedFor?: Record<number, unknown>; list?: unknown[]; openedReject?: boolean; chatTools?: boolean; provider?: string; newChatReject?: boolean; continued?: Promise<boolean> } = {}) {
  let entries = (opts.list ?? [OLD]) as Array<typeof OLD>;
  let next = 0;
  invoke.mockImplementation((cmd: string, args?: { id?: number }) => {
    if (cmd === 'model_settings') return Promise.resolve({ ...SETTINGS_READY, roles: { embed: opts.provider ?? 'openRouter', chat: opts.provider ?? 'openRouter' } });
    if (cmd === 'provider_status') return providerReply().then((s) => ({ embed: s, chat: s }));
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'list_history') return Promise.resolve(entries);
    if (cmd === 'chat_tools_enabled') return Promise.resolve(opts.chatTools ?? false);
    if (cmd === 'new_chat' && opts.newChatReject) return Promise.reject('boom');
    if (cmd === 'delete_history_entry') { entries = entries.filter((e) => e.id !== args?.id); return Promise.resolve(); }
    if (cmd === 'history_entry') {
      if (opts.openedReject) return Promise.reject('offline');
      const opened = opts.openedFor?.[args!.id!] ?? opts.opened;
      return opened instanceof Promise ? opened : Promise.resolve(opened);
    }
    if (cmd === 'continue_history') return opts.continued ?? Promise.resolve(true);
    if (cmd === 'source_around') return Promise.resolve(excerptSpanA);
    if (cmd === 'ask') {
      if (opts.ask) return opts.ask;
      if (next >= (opts.asks ?? []).length) throw new Error(`mockRecents: no reply for ask #${next + 1}`);
      const reply = opts.asks![next++];
      return reply instanceof Promise ? reply : Promise.resolve(reply);
    }
    return Promise.resolve();
  });
}
const historyEntryCalls = () => invoke.mock.calls.filter((c) => c[0] === 'history_entry');
const listHistoryCalls = () => invoke.mock.calls.filter((c) => c[0] === 'list_history');
const OPENED_OLD = { kind: 'dialogue', turns: [{ query: 'old q', answer: generated }] };

async function openRecent(id: number) {
  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId(`history-open-${id}`));
}

test('opening a question draws its snapshot without asking', async () => {
  mockRecents({ asks: [generated], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  const box = screen.getByRole('textbox') as HTMLInputElement;
  await fireEvent.input(box, { target: { value: 'a draft' } });
  const asks = askCalls().length;
  const answered = answeredCalls().length;

  await openRecent(7);

  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('old q'));
  expect(historyEntryCalls()).toEqual([['history_entry', { id: 7 }]]);
  expect(askCalls()).toHaveLength(asks);
  expect(answeredCalls()).toHaveLength(answered + 1);
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBe('true');
  expect(box.value).toBe('a draft');
});

test('a cold launcher warms on an opened question', async () => {
  mockRecents({ asks: [emptyCitationsOnly], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(true);
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');

  await openRecent(7);

  await screen.findByTestId('card-source');
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(false);
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('lsr');
  expect(invoke.mock.calls.filter((c) => c[0] === 'source_around')[0][1]).toMatchObject({ chunkId: 42 });
  await waitFor(() => expect(layoutCalls().at(-1)).toEqual(ON));
});

test('opening is ignored while an ask is in flight', async () => {
  mockRecents({ ask: new Promise(() => {}) });
  render(Launcher);
  await submit('slow');
  await waitFor(() => expect(askCalls()).toHaveLength(1));
  await openRecent(7);
  expect(historyEntryCalls()).toHaveLength(0);
  expect(screen.queryByTestId('card-centre')).toBeNull();
});

test('an unreadable entry keeps the answer on screen and says so', async () => {
  mockRecents({ asks: [generated], opened: { kind: 'unreadable' } });
  render(Launcher);
  await submit('first');
  const before = (await screen.findByTestId('card-centre')).textContent;
  await openRecent(7);
  await screen.findByRole('alert');
  expect(screen.getByRole('alert').textContent).toMatch(/cannot be opened|не відкривається/);
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('query-echo').textContent).toBe('first');
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

test('a gone entry rereads the list and changes nothing else', async () => {
  mockRecents({ asks: [generated], opened: { kind: 'gone' } });
  render(Launcher);
  await submit('first');
  const before = (await screen.findByTestId('card-centre')).textContent;
  const reads = listHistoryCalls().length;
  await openRecent(7);
  await waitFor(() => expect(listHistoryCalls()).toHaveLength(reads + 1));
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(screen.getByTestId('query-echo').textContent).toBe('first');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

test('a rejected open keeps the answer and says the query failed', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockRecents({ asks: [generated], openedReject: true });
  render(Launcher);
  await submit('first');
  const before = (await screen.findByTestId('card-centre')).textContent;
  await openRecent(7);
  await screen.findByRole('alert');
  expect(screen.getByRole('alert').textContent).toMatch(/could not|не вдалося/i);
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(err).toHaveBeenCalled();
  err.mockRestore();
});

test('deleting the question on screen keeps the answer and removes only its row', async () => {
  mockRecents({ asks: [generated], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('old q'));
  const before = screen.getByTestId('card-centre').textContent;
  const asks = askCalls().length;

  await fireEvent.click(screen.getByTestId('history-remove-7'));

  await waitFor(() => expect(screen.queryByTestId('tree-history-7')).toBeNull());
  expect(screen.getByTestId('card-centre').textContent).toBe(before);
  expect(askCalls()).toHaveLength(asks);
  // The × does not open the row it sits in: only the first open read an entry.
  expect(historyEntryCalls()).toHaveLength(1);
});

test('a recorded answer bumps the history list; a refusal does not', async () => {
  mockRecents({ asks: [generated, refusedNoCandidates] });
  render(Launcher);
  await submit('one');
  await screen.findByTestId('card-centre');
  await waitFor(() => expect(listHistoryCalls()).toHaveLength(2));
  await submit('two');
  await screen.findByRole('status');
  expect(listHistoryCalls()).toHaveLength(2);
});

test('a new recorded answer clears the mark on the question that was opened', async () => {
  mockRecents({ asks: [generated, generated], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBe('true'));
  await submit('second');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  await waitFor(() => expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull());
});

test('an opened answer without citations opens cold and draws no source card', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockRecents({ asks: [emptyCitationsOnly], opened: { kind: 'dialogue', turns: [{ query: 'old q', answer: emptyCitationsOnly }] } });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBe('true'));
  expect(screen.getByTestId('card-centre')).toBeTruthy();
  expect(screen.queryByTestId('card-source')).toBeNull();
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(err).not.toHaveBeenCalled();
  err.mockRestore();
});

test('launcher-cold clears the mark with the answer it belonged to', async () => {
  mockRecents({ asks: [refusedNoCandidates, refusedNoCandidates], opened: OPENED_OLD });
  render(Launcher);
  await waitFor(() => expect(cold.names).toContain('launcher-cold'));
  await submit('first');
  await screen.findByRole('status');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBe('true'));
  fireCold();
  await waitFor(() => expect(screen.queryByTestId('card-centre')).toBeNull());
  await submit('again');
  await screen.findByRole('status');
  // The tree remounted on the Files tab after the cold reset: Recents again.
  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

test('a search started while an entry is read is not overwritten by the entry', async () => {
  let resolveOpen!: (v: unknown) => void;
  mockRecents({ asks: [generated, new Promise(() => {})], opened: new Promise((r) => { resolveOpen = r; }) });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await submit('second');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  resolveOpen(OPENED_OLD);
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.getByRole('textbox').getAttribute('aria-busy')).toBe('true');
  expect(screen.queryByTestId('query-echo')).toBeNull();
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

test('launcher-cold while an entry is read drops it', async () => {
  let resolveOpen!: (v: unknown) => void;
  mockRecents({ asks: [generated], opened: new Promise((r) => { resolveOpen = r; }) });
  render(Launcher);
  await waitFor(() => expect(cold.names).toContain('launcher-cold'));
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  fireCold();
  resolveOpen(OPENED_OLD);
  await new Promise((r) => setTimeout(r, 0));
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(document.querySelector('main')!.getAttribute('data-cols')).toBe('s');
});

test('a refused search takes the mark off the opened question and does not reread Recents', async () => {
  mockRecents({ asks: [generated, refusedNoCandidates], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBe('true'));
  const reads = listHistoryCalls().length;

  await submit('second');
  await screen.findByRole('status');

  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
  expect(listHistoryCalls()).toHaveLength(reads);
});

test('an entry read that lands after a search has answered is dropped, not drawn over it', async () => {
  let resolveOpen!: (v: unknown) => void;
  mockRecents({ asks: [generated, generated], opened: new Promise((r) => { resolveOpen = r; }) });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);

  await submit('second');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('second'));
  resolveOpen(OPENED_OLD);
  await new Promise((r) => setTimeout(r, 0));

  expect(screen.getByTestId('query-echo').textContent).toBe('second');
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

test('two opens that resolve out of order: the later click wins', async () => {
  const NEWER = { id: 8, query: 'newer q', askedAt: 1_700_000_100 };
  const NEWER_ANSWER = { kind: 'dialogue', turns: [{ query: 'newer q', answer: generated }] };
  let resolveFirst!: (v: unknown) => void;
  let resolveSecond!: (v: unknown) => void;
  mockRecents({
    asks: [generated],
    list: [NEWER, OLD],
    openedFor: {
      7: new Promise((r) => { resolveFirst = r; }),
      8: new Promise((r) => { resolveSecond = r; }),
    },
  });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId('history-open-7'));
  await fireEvent.click(await screen.findByTestId('history-open-8'));

  resolveSecond(NEWER_ANSWER);
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('newer q'));
  resolveFirst(OPENED_OLD);
  await new Promise((r) => setTimeout(r, 0));

  expect(screen.getByTestId('query-echo').textContent).toBe('newer q');
  expect(screen.getByTestId('tree-history-8').getAttribute('aria-current')).toBe('true');
  expect(screen.getByTestId('tree-history-7').getAttribute('aria-current')).toBeNull();
});

// Task 8: the launcher draws a conversation. `chat_tools_enabled` is the «Tool
// support» switch (ruling R4): off keeps today's one-turn launcher.
const callsOf = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);
const echoes = () => screen.queryAllByTestId('query-echo').map((e) => e.textContent);
async function askTwice(chatTools: boolean) {
  mockRecents({ chatTools, asks: [generated, generatedOther] });
  render(Launcher);
  if (chatTools) await screen.findByTestId('new-chat');
  else await waitFor(() => expect(callsOf('chat_tools_enabled')).toHaveLength(1));
  await submit('first');
  await screen.findByTestId('preview-3');
  await submit('second');
  await screen.findByTestId('preview-2');
}

test('with tool support on, two asks draw two turns in order, each with its own previews', async () => {
  await askTwice(true);
  expect(echoes()).toEqual(['first', 'second']);
  expect(screen.getByTestId('preview-3')).toBeTruthy(); // the first turn's, still there
  expect(screen.getByTestId('preview-2')).toBeTruthy();
  expect(screen.getAllByTestId('card-centre')).toHaveLength(1);
});

test('a citation of the first turn opens the first turn\'s passage', async () => {
  await askTwice(true);
  await waitFor(() => expect(callsOf('source_around').at(-1)![1]).toMatchObject({ chunkId: 61 })); // the latest turn is selected
  const before = callsOf('source_around').length;
  await fireEvent.click(screen.getByTestId('preview-3'));
  // The siblings in the same document load after the clicked one, so look at all the new calls.
  await waitFor(() => expect(callsOf('source_around').slice(before).map((c) => c[1].chunkId)).toContain(42));
  // ...and its own sibling in the same document (43), not the latest turn's citations.
  await waitFor(() => expect(callsOf('source_around').slice(before).map((c) => c[1].chunkId)).toContain(43));
});

test('with tool support off there is no New chat button and the second ask replaces the first turn', async () => {
  await askTwice(false);
  expect(screen.queryByTestId('new-chat')).toBeNull();
  expect(echoes()).toEqual(['second']);
  expect(screen.queryByTestId('preview-3')).toBeNull();
});

test('with tool support off the first turn leaves the screen while the second ask is in flight', async () => {
  mockRecents({ chatTools: false, asks: [generated] });
  render(Launcher);
  await waitFor(() => expect(callsOf('chat_tools_enabled')).toHaveLength(1));
  await submit('first');
  await screen.findByTestId('preview-3');
  mockRecents({ chatTools: false, ask: new Promise(() => {}) });
  await submit('second');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  expect(screen.queryByTestId('preview-3')).toBeNull();
});

test('New chat calls new_chat, empties the centre and leaves the search line alone', async () => {
  await askTwice(true);
  const box = screen.getByRole('textbox') as HTMLInputElement;
  await fireEvent.input(box, { target: { value: 'a draft' } });
  await fireEvent.click(screen.getByTestId('new-chat'));
  await waitFor(() => expect(callsOf('new_chat')).toHaveLength(1));
  expect(echoes()).toEqual([]);
  expect(screen.queryByTestId('preview-3')).toBeNull();
  expect(screen.queryByTestId('preview-2')).toBeNull();
  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(box.value).toBe('a draft');
  // The next ask starts the new conversation: one turn, not three.
  mockRecents({ chatTools: true, asks: [generated] });
  await submit('third');
  await screen.findByTestId('preview-3');
  expect(echoes()).toEqual(['third']);
});

test('under Mnema with the switch on the launcher shows one turn and no New chat', async () => {
  mockRecents({ chatTools: true, provider: 'mnema', asks: [generated, generatedOther] });
  render(Launcher);
  await waitFor(() => expect(callsOf('chat_tools_enabled')).toHaveLength(1));
  await waitFor(() => expect(callsOf('provider_status')).toHaveLength(1));
  await submit('first');
  await screen.findByTestId('preview-3');
  await submit('second');
  await screen.findByTestId('preview-2');
  expect(echoes()).toEqual(['second']);
  expect(screen.queryByTestId('new-chat')).toBeNull();
});

test('New chat keeps the panels: the left one stays, the centre and right ones are empty', async () => {
  await askTwice(true);
  await fireEvent.click(screen.getByTestId('new-chat'));
  await waitFor(() => expect(echoes()).toEqual([]));
  expect(screen.getByTestId('card-tree').hasAttribute('hidden')).toBe(false);
  expect(screen.getByTestId('card-results-empty')).toBeTruthy();
  expect(screen.getByTestId('card-source-empty')).toBeTruthy();
});

test('a failed new_chat keeps the screen as it was', async () => {
  mockRecents({ chatTools: true, newChatReject: true, asks: [generated] });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('first');
  await screen.findByTestId('preview-3');
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  await fireEvent.click(screen.getByTestId('new-chat'));
  await waitFor(() => expect(callsOf('new_chat')).toHaveLength(1));
  await waitFor(() => expect(err).toHaveBeenCalled());
  expect(echoes()).toEqual(['first']);
  expect(screen.getByTestId('preview-3')).toBeTruthy();
  err.mockRestore();
});

test('New chat is disabled while an ask is in flight', async () => {
  mockRecents({ chatTools: true, ask: new Promise(() => {}) });
  render(Launcher);
  const button = (await screen.findByTestId('new-chat')) as HTMLButtonElement;
  expect(button.disabled).toBe(false);
  await submit('slow');
  await waitFor(() => expect(askCalls()).toHaveLength(1));
  expect(button.disabled).toBe(true);
  await fireEvent.click(button);
  expect(callsOf('new_chat')).toHaveLength(0);
});

test('the switch is read again when the window gains focus', async () => {
  mockRecents({ chatTools: false });
  render(Launcher);
  await waitFor(() => expect(callsOf('chat_tools_enabled')).toHaveLength(1));
  expect(screen.queryByTestId('new-chat')).toBeNull();
  mockRecents({ chatTools: true });
  await fireEvent.focus(window);
  await screen.findByTestId('new-chat');
  expect(callsOf('chat_tools_enabled')).toHaveLength(2);
});

test('launcher-cold forgets the turns', async () => {
  await askTwice(true);
  fireCold();
  await waitFor(() => expect(echoes()).toEqual([]));
  mockRecents({ chatTools: true, asks: [generated] });
  await submit('again');
  await screen.findByTestId('preview-3');
  expect(echoes()).toEqual(['again']);
});

test('opening a stored conversation draws every turn and the next ask adds a third', async () => {
  const opened = { kind: 'dialogue', turns: [{ query: 'old q', answer: generated }, { query: 'old q2', answer: generatedOther }] };
  mockRecents({ chatTools: true, opened, asks: [generated] });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('first'); // warm the launcher so Recents is reachable
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(echoes()).toEqual(['old q', 'old q2']));
  mockRecents({ chatTools: true, opened, asks: [{ ...generatedOther }] });
  await submit('third');
  await waitFor(() => expect(echoes()).toEqual(['old q', 'old q2', 'third']));
});

test('a failed turn is drawn as the current one and goes after the next good answer', async () => {
  const failed = { ...citationsOnly, why: { kind: 'noReply' } };
  mockRecents({ chatTools: true, asks: [generated, failed, generatedOther] });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('first');
  await screen.findByTestId('preview-3');
  await submit('second');
  await screen.findByTestId('citations-banner');
  expect(screen.getByTestId('preview-3')).toBeTruthy(); // the good turn stays above it
  await submit('third');
  await screen.findByTestId('preview-2');
  expect(screen.queryByTestId('citations-banner')).toBeNull();
  expect(echoes()).toEqual(['first', 'third']);
});

test('a no-tool-use answer says so in the banner, in both languages', async () => {
  const noTools = { ...citationsOnly, why: { kind: 'noToolUse' } };
  try {
    mockRecents({ chatTools: true, asks: [noTools] });
    render(Launcher);
    await submit('q');
    const banner = await screen.findByTestId('citations-banner');
    expect(banner.textContent).toContain('This model cannot continue a conversation. Choose another one or turn off Tool support.');
    setLocale('uk');
    await waitFor(() => expect(screen.getByTestId('citations-banner').textContent).toContain('Ця модель не підтримує продовження розмови. Виберіть іншу або вимкніть «Підтримку інструментів».'));
  } finally { setLocale('en'); }
});

// The backend session changes only for the open the screen draws, and every
// call that changes it reaches the backend in the order the person acted.
const continueCalls = () => invoke.mock.calls.filter((c) => c[0] === 'continue_history');
const OLD8 = { id: 8, query: 'second q', askedAt: 1_700_000_100 };
const OPENED_8 = { kind: 'dialogue', turns: [{ query: 'second q', answer: generatedOther }] };

test('of two overlapping opens only the one drawn continues the backend session', async () => {
  let slow!: (v: unknown) => void;
  const first = new Promise((r) => { slow = r; });
  mockRecents({ chatTools: true, asks: [generated], list: [OLD, OLD8], openedFor: { 7: first, 8: OPENED_8 } });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('warm');
  await screen.findByTestId('card-centre');
  await fireEvent.click(await screen.findByTestId('tree-tab-recents'));
  await fireEvent.click(await screen.findByTestId('history-open-7'));
  await fireEvent.click(await screen.findByTestId('history-open-8'));
  await waitFor(() => expect(echoes()).toEqual(['second q']));
  slow(OPENED_OLD); // the stale read lands after the newer open is drawn
  await waitFor(() => expect(historyEntryCalls()).toHaveLength(2));
  await new Promise((r) => setTimeout(r, 0));
  expect(continueCalls()).toEqual([['continue_history', { id: 8 }]]);
  expect(echoes()).toEqual(['second q']);
});

test('an open that a submit superseded never continues the backend session', async () => {
  let slow!: (v: unknown) => void;
  const read = new Promise((r) => { slow = r; });
  mockRecents({ chatTools: true, asks: [generated, generatedOther], opened: read });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('warm');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(historyEntryCalls()).toHaveLength(1));
  await submit('typed meanwhile');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  slow(OPENED_OLD);
  await new Promise((r) => setTimeout(r, 0));
  expect(continueCalls()).toHaveLength(0);
});

test('an open continues the session before the next ask reaches the backend', async () => {
  mockRecents({ chatTools: true, asks: [generated, generatedOther], opened: OPENED_OLD });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('warm');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(echoes()).toEqual(['old q']));
  await submit('next');
  await waitFor(() => expect(askCalls()).toHaveLength(2));
  const order = invoke.mock.calls.map((c) => c[0]).filter((c) => c === 'ask' || c === 'continue_history' || c === 'new_chat');
  expect(order).toEqual(['ask', 'continue_history', 'ask']);
});

test('with conversations off an open does not touch the backend session', async () => {
  mockRecents({ chatTools: false, asks: [generated], opened: OPENED_OLD });
  render(Launcher);
  await submit('first');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('old q'));
  expect(continueCalls()).toHaveLength(0);
});

test('New chat waits for a continue that is still on its way to the backend', async () => {
  let done!: (v: boolean) => void;
  const continued = new Promise<boolean>((r) => { done = r; });
  mockRecents({ chatTools: true, asks: [generated], opened: OPENED_OLD, continued });
  render(Launcher);
  await screen.findByTestId('new-chat');
  await submit('warm');
  await screen.findByTestId('card-centre');
  await openRecent(7);
  await waitFor(() => expect(continueCalls()).toHaveLength(1));
  await fireEvent.click(screen.getByTestId('new-chat'));
  await new Promise((r) => setTimeout(r, 0));
  expect(callsOf('new_chat')).toHaveLength(0);
  done(true);
  await waitFor(() => expect(callsOf('new_chat')).toHaveLength(1));
});

// The cloud is ok only when the backend's probe is ok AND `roleReady` holds, the
// rule the settings tab dots read (Models.test.ts has the other half).
test('the_tab_dot_and_launcher_agree', async () => {
  for (const f of READINESS_FIXTURES) {
    expect(roleReady(f.settings, 'embed'), f.name).toBe(f.embedReady);
    expect(roleReady(f.settings, 'chat'), f.name).toBe(f.chatReady);
    mockBackend(undefined);
    const base = invoke.getMockImplementation()!; // the probe says ok for both roles
    invoke.mockImplementation((cmd: string, ...a: unknown[]) =>
      cmd === 'model_settings' ? Promise.resolve(f.settings) : base(cmd, ...a));
    render(Launcher);
    const want = f.embedReady && f.chatReady ? 'ok' : 'notConfigured';
    await waitFor(() => expect(cloud().getAttribute('data-status'), f.name).toBe(want));
    cleanup();
  }
});

test('a failed settings read leaves the cloud not ready, whatever the probe said', async () => {
  mockBackend(undefined);
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation((cmd: string, ...a: unknown[]) =>
    cmd === 'model_settings' ? Promise.reject(new Error('down')) : base(cmd, ...a));
  render(Launcher);
  await waitFor(() => expect(cloud().getAttribute('data-status')).toBe('notConfigured'));
});
