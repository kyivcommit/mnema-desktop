import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import Recents from './Recents.svelte';
import { historyTwoQuestions } from '../lib/fixtures';
import { setLocale } from '../i18n';
import type { HistoryEntry } from '../lib/ipc';

// The house pattern for the bridge (`lib/ipc.test.ts:14-15`): one shared spy,
// the module mocked once at file scope.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

let history: HistoryEntry[] = [];
let failList = false;
let failDelete = false;
function route(cmd: string, args?: unknown): Promise<unknown> {
  if (cmd === 'list_history') {
    return failList
      ? Promise.reject(new Error('list_history failed'))
      : Promise.resolve(history.map((h) => ({ ...h })));
  }
  if (cmd === 'delete_history_entry') {
    if (failDelete) return Promise.reject(new Error('delete_history_entry failed'));
    const { id } = args as { id: number };
    history = history.filter((h) => h.id !== id);
    return Promise.resolve();
  }
  throw new Error(`unexpected command ${cmd}`);
}

function mockHistory(entries: HistoryEntry[]) {
  history = entries.map((e) => ({ ...e }));
}
function historyCalls() {
  return invoke.mock.calls.filter((c) => c[0] === 'list_history');
}
function rowPart(id: number, part: 'recents-query' | 'recents-asked') {
  return screen.getByTestId(`recents-row-${id}`).querySelector(`[data-testid="${part}"]`)!.textContent;
}

// The clock is pinned so the two fixture questions land on 4 and 5 minutes, which
// in Ukrainian are different plural arms (few and many).
const NOW = 1_700_000_340_000; // 240 s after the newer question, 340 s after the older

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(route);
  history = [];
  failList = false;
  failDelete = false;
  setLocale('en');
});

// `locale` is a module-level store shared by every test in this file, and a
// pinned clock outlives a failed assertion: both are restored unconditionally.
afterEach(() => {
  setLocale('en');
  vi.restoreAllMocks();
});

test('the row shows the first question and the relative time', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW);
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-1')).toBeTruthy();
  // The order is the order `list_history` sent, newest first: the list does not sort.
  expect(screen.getAllByTestId(/^recents-row-/).map((el) => el.dataset.testid))
    .toEqual(['recents-row-2', 'recents-row-1']);
  expect([
    rowPart(2, 'recents-query'),
    rowPart(2, 'recents-asked'),
    rowPart(1, 'recents-query'),
    rowPart(1, 'recents-asked'),
  ]).toEqual([
    'when does the lease renew',
    '4 minutes ago',
    'invoice payment terms',
    '5 minutes ago',
  ]);
});

test('× deletes and does not open', async () => {
  mockHistory(historyTwoQuestions);
  const onOpen = vi.fn();
  const onDelete = vi.fn();
  render(Recents, { activeId: null, gen: 0, onOpen, onDelete });

  await fireEvent.click(await screen.findByTestId('recents-remove-1'));

  expect(invoke).toHaveBeenCalledWith('delete_history_entry', { id: 1 });
  await waitFor(() => expect(screen.queryByTestId('recents-row-1')).toBeNull());
  expect(screen.getByTestId('recents-row-2')).toBeTruthy();
  // It is a sibling of the open button, not a child: the click reaches one.
  expect(onOpen).not.toHaveBeenCalled();
  expect(onDelete.mock.calls).toEqual([[1]]);
});

test('the recency follows a live language switch, through both plural arms', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW);
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
  expect(rowPart(2, 'recents-asked')).toBe('4 minutes ago');

  setLocale('uk');
  await tick();
  expect(rowPart(2, 'recents-asked')).toBe('4 хвилини тому'); // few
  expect(rowPart(1, 'recents-asked')).toBe('5 хвилин тому'); // many

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(rowPart(2, 'recents-asked')).toBe('4 minutes ago');
});

// The hazard the row ids are a namespace against: a second id inside a row would
// be counted as a row by anything querying `/^recents-row-/`, so the row's parts
// carry ids that do not start with that prefix.
test('the row namespace holds one id per question and nothing else', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-1')).toBeTruthy();
  expect(screen.getAllByTestId(/^recents-row-/).map((el) => el.dataset.testid))
    .toEqual(['recents-row-2', 'recents-row-1']);
});

// The conversation on screen is the one `historyId` names — the row a fresh
// answer wrote, or an opened one. `aria-current` is the attribute the style keys on.
test('the conversation on screen is the one row marked aria-current', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: 1, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-1')).toBeTruthy();
  expect(screen.getByTestId('recents-row-1').getAttribute('aria-current')).toBe('true');
  expect(screen.getByTestId('recents-row-2').getAttribute('aria-current')).toBeNull();
});

test('a click on a row opens it and does not ask again', async () => {
  mockHistory(historyTwoQuestions);
  const onOpen = vi.fn();
  render(Recents, { activeId: null, gen: 0, onOpen });

  await fireEvent.click(await screen.findByTestId('recents-open-1'));

  expect(onOpen.mock.calls).toEqual([[1]]);
  expect(invoke.mock.calls.filter((c) => c[0] === 'ask')).toEqual([]);
});

// Ruling N: an empty list is not an empty index, and it says so in its own words.
test('an empty list says so, in the words of the catalog', async () => {
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-empty')).toBeTruthy();
  expect(screen.getByTestId('recents-empty').textContent).toBe('Your questions will appear here');

  setLocale('uk');
  await tick();
  expect(screen.getByTestId('recents-empty').textContent).toBe('Тут з’являться ваші запити');
});

// A first read that fails says so, and does not draw an empty list that would
// read as "no questions".
test('a list that cannot be read says so', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  failList = true;
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-failed')).toBeTruthy();
  expect(screen.getByTestId('recents-failed').textContent).toBe('The conversations could not be loaded.');
  expect(screen.queryByTestId('recents-empty')).toBeNull();
  expect(err).toHaveBeenCalled(); // logged, so the rejection was handled, not left unhandled
});

// The remove button's accessible name is the only thing that tells two rows apart
// for a screen reader, so it is asserted by name, scoped to its own row, in both
// directions.
test('each remove button is named for its own question, so two rows can be told apart', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  const row1 = await screen.findByTestId('recents-row-1');
  const row2 = screen.getByTestId('recents-row-2');
  expect(within(row1).getByRole('button', { name: 'Remove from history: invoice payment terms' })).toBeTruthy();
  expect(within(row2).getByRole('button', { name: 'Remove from history: when does the lease renew' })).toBeTruthy();
  expect(within(row1).queryByRole('button', { name: 'Remove from history: when does the lease renew' })).toBeNull();
});

// A delete that fails must not take the row off the screen: the list is what the
// database still holds, and the person was not told the question is gone.
test('a delete that fails stays on screen and opens nothing', async () => {
  mockHistory(historyTwoQuestions);
  failDelete = true;
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  const onOpen = vi.fn();
  render(Recents, { activeId: null, gen: 0, onOpen });

  await fireEvent.click(await screen.findByTestId('recents-remove-1'));
  await waitFor(() => expect(err).toHaveBeenCalled());

  expect(screen.getByTestId('recents-row-1')).toBeTruthy();
  expect(screen.getByTestId('recents-row-2')).toBeTruthy();
  expect(onOpen).not.toHaveBeenCalled();
});

// The list is read when it mounts, when the window regains focus (the launcher
// is hidden, not closed, so nothing else would tell it), and when the launcher
// says a question was answered (`gen`).
test('the list is read on mount, again on focus, and again when gen changes', async () => {
  mockHistory(historyTwoQuestions);
  const { rerender } = render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
  expect(historyCalls()).toHaveLength(1);

  await fireEvent.focus(window);
  await waitFor(() => expect(historyCalls()).toHaveLength(2));

  // A question asked while the window is up: the list must show it at once.
  mockHistory([{ id: 3, query: 'a question just asked', askedAt: 1_700_000_300 }, ...historyTwoQuestions]);
  await rerender({ activeId: null, gen: 1, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-3')).toBeTruthy();
  expect(historyCalls()).toHaveLength(3);
});

// An open that failed was clicked here, and it is said here rather than under a
// composer the person did not touch.
test('a failed open says so beside the list', async () => {
  const { rerender } = render(Recents, { activeId: null, gen: 0, onOpen: () => {}, error: null });
  expect(screen.queryByTestId('recents-error')).toBeNull();

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: 'historyUnreadable' });
  expect((await screen.findByTestId('recents-error')).textContent).toBe('This entry cannot be opened.');

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: 'historyFailed' });
  expect((await screen.findByTestId('recents-error')).textContent).toBe('The query could not be run.');

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: null });
  await tick();
  expect(screen.queryByTestId('recents-error')).toBeNull();
});
