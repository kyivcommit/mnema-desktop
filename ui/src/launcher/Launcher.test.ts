import { render, screen, fireEvent } from '@testing-library/svelte';
import { vi, expect, test, beforeEach } from 'vitest';
import Launcher from './Launcher.svelte';
import { refusedNoCandidates, generated } from '../lib/fixtures';

const hide = vi.fn();
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ hide }) }));
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// Answers each command separately. `model_settings` (the launcher's mount
// seed) always resolves so it never crashes a render; `ask` is what each
// test controls.
const NO_PROVIDER = { key: { kind: 'absent' }, index: { kind: 'read', embeddingModel: null, searchTextArm: true, searchContentArm: false } };
function mockBackend(askReply: unknown, opts: { reject?: boolean } = {}) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'model_settings') return Promise.resolve(NO_PROVIDER);
    if (cmd === 'ask') return opts.reject ? Promise.reject(askReply) : Promise.resolve(askReply);
    return Promise.resolve();
  });
}
const askCalls = () => invoke.mock.calls.filter((c) => c[0] === 'ask');

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

test('on ready the line clears and the query echoes', async () => {
  mockBackend(refusedNoCandidates); // any successful answer clears+echoes; a refusal is one
  render(Launcher);
  await submit('echo me');
  await screen.findByRole('status');
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe(''); // line cleared
  expect(screen.getByTestId('query-echo').textContent).toBe('echo me');    // echoed
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

test('a generated answer renders the B stub, not a refusal', async () => {
  mockBackend(generated);
  render(Launcher);
  await submit('q');
  await screen.findByTestId('answer-stub');
  expect(screen.queryByRole('status')).toBeNull(); // not a refusal
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
