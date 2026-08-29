import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { vi, expect, test, beforeEach } from 'vitest';
import Launcher from './Launcher.svelte';
import { refusedNoCandidates, generated, oneRootTwoFolders } from '../lib/fixtures';

const hide = vi.fn();
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ hide }) }));
const invoke = vi.fn();
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
const NO_PROVIDER = { key: { kind: 'absent' }, index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: true, searchContentArm: false } };
function mockBackend(askReply: unknown, opts: { reject?: boolean } = {}) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(NO_PROVIDER);
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
    if (cmd === 'ask') return opts.reject ? Promise.reject(askReply) : Promise.resolve(askReply);
    return Promise.resolve();
  });
}
const askCalls = () => invoke.mock.calls.filter((c) => c[0] === 'ask');
const listTreeCalls = () => invoke.mock.calls.filter((c) => c[0] === 'list_tree');

// Answers each `ask` in turn, so one test can drive two questions with different
// outcomes — the refusal path (ruling I-B) needs a generated answer first and a
// refusal second. Everything else answers as `mockBackend` does.
function mockAsks(...replies: unknown[]) {
  let next = 0;
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(NO_PROVIDER);
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
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

// The arms-row seeds each need their OWN `model_settings` answer, which is why
// they cannot use `mockBackend`. They still need every other command answered:
// they render in state A today, where no card draws, so a blanket
// `Promise.resolve()` is green for a reason unrelated to what they claim — and
// one state along `Tree` reads `.roots` off `undefined` and throws. Same trap as
// Ruling Y, fourth home.
function mockSettings(settings: unknown) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(settings);
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
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
beforeEach(() => { hide.mockClear(); invoke.mockReset(); mockBackend(undefined); });

async function submit(value: string) {
  const box = screen.getByRole('textbox');
  await fireEvent.input(box, { target: { value } });
  await fireEvent.keyDown(box, { key: 'Enter' });
}

test('a blank query never reaches ask and shows the blank message', async () => {
  render(Launcher);
  await submit('   ');
  expect(askCalls()).toHaveLength(0); // model_settings may run on mount; ask must not
  expect(screen.getByRole('alert').textContent).toMatch(/query|запит/i);
});

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
    if (cmd === 'model_settings') return Promise.resolve(NO_PROVIDER);
    if (cmd === 'list_tree') return Promise.resolve(oneRootTwoFolders);
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

test('a generated answer renders the centre card, not a refusal', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('card-centre');
  expect(screen.queryByRole('status')).toBeNull(); // not a refusal
});

// 🔴 C1, and the only place it can be seen: `Cards.test.ts` drives `Cards` by
// hand and can build a transition the product never performs, while these tests
// drive the real `runSearch`. `Cards` gates its cards on the launcher's state
// and `runSearch` sets `inFlight` before EVERY ask (`Launcher.svelte:42`), so a
// tree drawn only for a generated answer is torn down and refetched in the
// middle of every question — the outcome Ruling AC forbids, reached with no
// `{#key}` anywhere. Both are anchored on the echo, which only a RESOLVED ask
// can write (`Launcher.svelte:53`).
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
// row F says «тихе повідомлення», which is no more "only the line" than row D's
// spinner is — and the reason the tree survives D (its content is the INDEX, not
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
// The gate keeps the tree for `error`, and `error` carries three reasons. The
// `Cards`-level test pins `reason: 'blank'`; these pin the transition the ruling
// was actually argued from — a person with three cards on screen mistyping an
// Enter — which is the half that makes "do not narrow the gate by reason"
// falsifiable. Anchored on the guard message, which only a completed validation
// writes (`SearchLine.svelte:46`).
test('a blank Enter from state B keeps the tree', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('   ');
  await screen.findByRole('alert');

  expect(screen.getByTestId('card-tree')).toBeTruthy();
});

test('a blank Enter from state B does not shut a hand-opened folder', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('   ');
  await screen.findByRole('alert');

  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

test('a blank Enter from state B drops the answer and source cards', async () => {
  mockBackend(generated);
  await askAndOpenAFolder();

  await submit('   ');
  await screen.findByRole('alert');

  expect(screen.queryByTestId('card-centre')).toBeNull();
  expect(screen.queryByTestId('card-source')).toBeNull();
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

test('the arms row seeds from model_settings — a present key and a chosen model enable content', async () => {
  mockSettings({ key: { kind: 'present' }, index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true } });
  render(Launcher);
  await vi.waitFor(() => {
    const content = (screen.getAllByRole('checkbox') as HTMLInputElement[])[1];
    expect(content.disabled).toBe(false); // seed applied: present key + chosen model enable content
  });
  expect(invoke).toHaveBeenCalledWith('model_settings');
});

// §9.1 / owner ruling 2026-08-24: the exact configuration the owner's live run hit — a stored
// provider key with no chosen embedding model. A key alone cannot embed a query, so content must
// stay disabled. Against the pre-fix `provider = s.key.kind === 'present'` this fails (content
// would wrongly enable on a present key alone) — that failure is the regression proof.
//
// `searchTextArm: false` here is not part of the ruling under test — it is a marker seeded away
// from `textOn`'s `true` default so the `waitFor` below cannot pass before `model_settings`
// resolves. `content.disabled` alone is already `true` in the pre-seed default (provider starts
// `false`), so asserting it directly would pass vacuously, seed or no seed.
test('the arms row seeds from model_settings — a present key with no chosen model leaves content disabled', async () => {
  mockSettings({ key: { kind: 'present' }, index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: false, searchContentArm: false } });
  render(Launcher);
  await vi.waitFor(() => {
    // Proves the seed actually ran before the real assertion below reads `provider`'s result.
    const text = (screen.getAllByRole('checkbox') as HTMLInputElement[])[0];
    expect(text.checked).toBe(false);
  });
  const content = (screen.getAllByRole('checkbox') as HTMLInputElement[])[1];
  expect(content.disabled).toBe(true); // no chosen model: content stays off despite a present key
  expect(invoke).toHaveBeenCalledWith('model_settings');
});

test('the arms row seeds from model_settings — searchTextArm:false unchecks the text arm', async () => {
  // The test above only proves the provider flag reached the row (content's
  // `disabled` depends solely on `provider`). This proves the arm *values*
  // flow too: `textOn` defaults to true, so an unchanged default would pass
  // silently — seeding false is the only way to catch a broken or renamed
  // `s.index.searchTextArm` read.
  mockSettings({ key: { kind: 'present' }, index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: 'text-embedding-3-small', searchTextArm: false, searchContentArm: true } });
  render(Launcher);
  await vi.waitFor(() => {
    const text = (screen.getAllByRole('checkbox') as HTMLInputElement[])[0];
    expect(text.checked).toBe(false); // seed applied: searchTextArm:false flowed to the checkbox
  });
  expect(invoke).toHaveBeenCalledWith('model_settings');
});
