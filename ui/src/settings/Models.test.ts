import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Models from './Models.svelte';
import { setLocale } from '../i18n';
import type { ModelSettings } from '../lib/ipc';

// Mocked in the shape Arms.test.ts:5-6 already uses — the typed wrappers, not
// the raw `invoke`.
const modelSettings = vi.fn();
const setKey = vi.fn();
const forgetKey = vi.fn();
vi.mock('../lib/ipc', () => ({
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  setKey: (...a: unknown[]) => setKey(...a),
  forgetKey: (...a: unknown[]) => forgetKey(...a),
}));

beforeEach(() => {
  modelSettings.mockReset();
  setKey.mockReset();
  forgetKey.mockReset();
});
afterEach(() => {
  cleanup();
  setLocale('en');
});

// One base fixture, overridden per test — the three axes (key, index,
// platform) are independent, so a test that needs one changed does not have
// to restate the other two.
function settings(overrides: Partial<ModelSettings> = {}): ModelSettings {
  return {
    key: { kind: 'absent' },
    index: { kind: 'read', embeddingModel: null, searchTextArm: true, searchContentArm: false },
    platform: 'linux',
    ...overrides,
  };
}

async function renderWith(s: ModelSettings) {
  setLocale('en'); // seed, do not inherit — the shape every Settings.test.ts test already uses
  modelSettings.mockResolvedValue(s);
  const result = render(Models);
  // The section fetches on mount; every test needs the settled DOM before
  // asserting on it.
  await waitFor(() => expect(modelSettings).toHaveBeenCalled());
  await Promise.resolve();
  await Promise.resolve();
  return result;
}

// ---------------------------------------------------------------------------
// Claim 0: the index Unreadable branch, and only that branch. `Unreadable`
// carries no `IndexRead` at all — the fixture question this task opens with —
// so a test that never gives the type an IndexRead field cannot pass by
// accident on a code path that quietly reads one.
// ---------------------------------------------------------------------------

test('index Unreadable/notOpen renders one sentence, never the reason', async () => {
  await renderWith(settings({
    index: { kind: 'unreadable', cause: 'notOpen', reason: 'LEAK-TOKEN-NOT-OPEN' },
  }));
  expect(screen.getByText('The index is not open yet.')).toBeTruthy();
  expect(screen.queryByText(/LEAK-TOKEN-NOT-OPEN/)).toBeNull();
});

test('index Unreadable/readFailed renders its own, different sentence, never the reason', async () => {
  await renderWith(settings({
    index: { kind: 'unreadable', cause: 'readFailed', reason: 'LEAK-TOKEN-READ-FAILED' },
  }));
  expect(screen.getByText('The index could not be read — this is a defect in this build.')).toBeTruthy();
  expect(screen.queryByText(/LEAK-TOKEN-READ-FAILED/)).toBeNull();
});

test('index Read renders no failure sentence at all', async () => {
  await renderWith(settings({
    index: { kind: 'read', embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true },
  }));
  expect(screen.queryByText('The index is not open yet.')).toBeNull();
  expect(screen.queryByText('The index could not be read — this is a defect in this build.')).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 1: Absent shows an empty key field and an add-a-key affordance;
// Present shows a masked value and the two buttons, and no key characters —
// there is none to show (models.rs:150-162).
// ---------------------------------------------------------------------------

test('key Absent: an empty field to add a key, no Change/Forget', async () => {
  await renderWith(settings({ key: { kind: 'absent' } }));
  const input = screen.getByLabelText('Key') as HTMLInputElement;
  expect(input.value).toBe('');
  expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Change' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Forget' })).toBeNull();
});

test('key Present: a masked value, Change and Forget, no editable field', async () => {
  await renderWith(settings({ key: { kind: 'present' } }));
  expect(screen.getByText('••••••••')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy();
  expect(screen.queryByLabelText('Key')).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 2: three of the four Unreadable causes name the action their own doc
// names; the fourth (Refused) says there is none. No two show the same text,
// and `reason` never reaches the screen.
// ---------------------------------------------------------------------------

const KEY_FAILURE_SENTENCES: Record<'locked' | 'duplicate' | 'refused' | 'defect', string> = {
  locked: 'The credential store did not answer: it may be locked, or a permission prompt was declined.',
  duplicate: 'More than one credential is filed under this installation. Remove the duplicate in the system credential store.',
  refused: 'The credential store refused to answer. This build cannot tell what to do next.',
  defect: 'This is a defect in this build, not a state of your system. Please report it to the developers.',
};

for (const cause of ['locked', 'duplicate', 'refused', 'defect'] as const) {
  test(`key Unreadable/${cause} shows its own sentence and never the reason`, async () => {
    await renderWith(settings({
      key: { kind: 'unreadable', cause, reason: `LEAK-TOKEN-${cause.toUpperCase()}` },
    }));
    expect(screen.getByText(KEY_FAILURE_SENTENCES[cause])).toBeTruthy();
    expect(screen.queryByText(new RegExp(`LEAK-TOKEN-${cause.toUpperCase()}`))).toBeNull();
  });
}

test('no two of the four Unreadable causes render the same sentence', () => {
  const texts = Object.values(KEY_FAILURE_SENTENCES);
  expect(new Set(texts).size).toBe(texts.length);
});

// ---------------------------------------------------------------------------
// Claim 3: the mac keychain note is platform-specific — present only there,
// absent on the other two, asserted in both directions.
// ---------------------------------------------------------------------------

const MAC_NOTE = 'Every update makes this application a stranger to its own key: the system will ask once for your login keychain password.';

test('platform mac renders the keychain note', async () => {
  await renderWith(settings({ platform: 'mac' }));
  expect(screen.getByText(MAC_NOTE)).toBeTruthy();
});

test('platform windows does not render the keychain note', async () => {
  await renderWith(settings({ platform: 'windows' }));
  expect(screen.queryByText(MAC_NOTE)).toBeNull();
});

test('platform linux does not render the keychain note', async () => {
  await renderWith(settings({ platform: 'linux' }));
  expect(screen.queryByText(MAC_NOTE)).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 4: Forget calls forget_key and re-reads model_settings; Removed and
// NothingToRemove say different things.
// ---------------------------------------------------------------------------

test('Forget calls forget_key, re-reads model_settings, and Removed says so', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }));
  forgetKey.mockResolvedValue({ kind: 'removed' });

  render(Models);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));

  await waitFor(() => expect(forgetKey).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(modelSettings).toHaveBeenCalledTimes(2)); // mount + the re-read Forget triggers
  await waitFor(() => expect(screen.getByText('The key was removed.')).toBeTruthy());
});

test('Forget calls forget_key, re-reads model_settings, and NothingToRemove says a different thing', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }));
  forgetKey.mockResolvedValue({ kind: 'nothingToRemove' });

  render(Models);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Forget' })).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Forget' }));

  await waitFor(() => expect(forgetKey).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(screen.getByText('There was no key to remove.')).toBeTruthy());
  expect(screen.queryByText('The key was removed.')).toBeNull();
});

// ---------------------------------------------------------------------------
// Claim 5 / Step 5: entering a key calls set_key with it, and after the round
// completes — set_key, then the re-read model_settings — no rendered text and
// no component state contains the entered key. A distinctive fixture value
// makes a leak unmistakable.
// ---------------------------------------------------------------------------

const LEAKY_KEY = 'sk-or-DO-NOT-LEAK-9f2a71';

test('entering a key calls set_key, and no trace of it survives the round', async () => {
  setLocale('en');
  modelSettings
    .mockResolvedValueOnce(settings({ key: { kind: 'absent' } }))
    .mockResolvedValueOnce(settings({ key: { kind: 'present' } }));
  setKey.mockResolvedValue({ balance: { kind: 'notStated' } });

  const { container } = render(Models);
  await waitFor(() => expect(screen.getByLabelText('Key')).toBeTruthy());

  await fireEvent.input(screen.getByLabelText('Key'), { target: { value: LEAKY_KEY } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

  await waitFor(() => expect(setKey).toHaveBeenCalledWith(LEAKY_KEY));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Change' })).toBeTruthy());

  // No rendered text contains it …
  expect(container.innerHTML).not.toContain(LEAKY_KEY);

  // … and no component state does either: reopening the editor must show an
  // empty field, not the value that was just sent.
  await fireEvent.click(screen.getByRole('button', { name: 'Change' }));
  const reopened = screen.getByLabelText('Key') as HTMLInputElement;
  expect(reopened.value).toBe('');
});

// ---------------------------------------------------------------------------
// Step 6: read the whole rendered section, as a person, in both directions —
// the everything-green state and the everything-red state.
// ---------------------------------------------------------------------------

test('reads as a person: everything configured, nothing alarming shown', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'present' },
    index: { kind: 'read', embeddingModel: 'text-embedding-3-small', searchTextArm: true, searchContentArm: true },
    platform: 'linux',
  }));
  const text = container.textContent ?? '';
  expect(text).toContain('OpenRouter');
  expect(text).toContain('••••••••');
  expect(text).toContain('Change');
  expect(text).toContain('Forget');
  expect(text).not.toContain('defect');
  expect(text).not.toContain('not open');
});

test('reads as a person: a locked keychain names the situation, not a status code', async () => {
  const { container } = await renderWith(settings({
    key: { kind: 'unreadable', cause: 'locked', reason: 'errSecInteractionNotAllowed -25308' },
    platform: 'mac',
  }));
  const text = container.textContent ?? '';
  expect(text).toContain('The credential store did not answer: it may be locked, or a permission prompt was declined.');
  expect(text).not.toContain('errSecInteractionNotAllowed');
  expect(text).not.toContain('-25308');
});
