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
const NO_PROVIDER = { key: { kind: 'absent' }, index: { kind: 'read', embeddingModel: null, searchTextArm: true, searchContentArm: false } };
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

// 🔴 C1, and the only place it can be seen. `Cards` gates its cards on the
// launcher's state and `runSearch` sets `inFlight` before EVERY ask
// (`Launcher.svelte:42`), so a tree drawn only for a generated answer is torn
// down and refetched in the middle of every question — the outcome Ruling AC
// forbids, reached with no `{#key}` anywhere. `Cards.test.ts` cannot catch it:
// its `rerender`s can build a transition the product never performs. This test
// drives the real `runSearch` twice and is anchored on the echo, which only a
// RESOLVED ask can write (`Launcher.svelte:50`).
test('a second question neither refetches the tree nor shuts a hand-opened folder', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('first question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('first question'));

  await fireEvent.click(await screen.findByTestId('tree-folder-archive')); // opened by hand
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
  expect(listTreeCalls()).toHaveLength(1);

  await submit('second question');
  await waitFor(() => expect(screen.getByTestId('query-echo').textContent).toBe('second question'));

  expect(listTreeCalls()).toHaveLength(1);
  expect(screen.getByTestId('tree-folder-archive').getAttribute('aria-expanded')).toBe('true');
});

test('a second submit while in flight is ignored — one ask at a time', async () => {
  mockBackend(new Promise(() => {})); // ask never resolves (Promise.resolve of a pending promise stays pending) → in flight
  render(Launcher);
  await submit('first');
  await submit('second');
  expect(askCalls()).toHaveLength(1); // the second Enter did not start a second ask
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

test('a pinned launcher ignores click-outside (blur) — the pin disables it', async () => {
  render(Launcher);
  await fireEvent.click(screen.getByRole('button', { name: /pin|пін|📌/i }));
  hide.mockClear();
  await fireEvent.blur(window);
  expect(hide).not.toHaveBeenCalled();
});

test('the arms row seeds from model_settings — a present key and a chosen model enable content', async () => {
  invoke.mockImplementation((cmd: string) =>
    cmd === 'model_settings'
      ? Promise.resolve({ key: { kind: 'present' }, index: { kind: 'read', embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true } })
      : Promise.resolve());
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
  invoke.mockImplementation((cmd: string) =>
    cmd === 'model_settings'
      ? Promise.resolve({ key: { kind: 'present' }, index: { kind: 'read', embeddingModel: null, searchTextArm: false, searchContentArm: false } })
      : Promise.resolve());
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
  invoke.mockImplementation((cmd: string) =>
    cmd === 'model_settings'
      ? Promise.resolve({ key: { kind: 'present' }, index: { kind: 'read', embeddingModel: 'text-embedding-3-small', searchTextArm: false, searchContentArm: true } })
      : Promise.resolve());
  render(Launcher);
  await vi.waitFor(() => {
    const text = (screen.getAllByRole('checkbox') as HTMLInputElement[])[0];
    expect(text.checked).toBe(false); // seed applied: searchTextArm:false flowed to the checkbox
  });
  expect(invoke).toHaveBeenCalledWith('model_settings');
});
