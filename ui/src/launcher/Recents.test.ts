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
// The stored answer each entry holds — longer than `preview`, which is one
// clipped line of it. A filter is matched over this, never over the preview.
let answers: Record<number, string> = {};
let failList = false;
// Reject the read asked for with this exact filter, and only it: the two-read
// races below need one failing reply and one succeeding one at once.
let failFilter: string | null = null;
let failDelete = false;
// When set, every `list_history` parks here until the test calls its release:
// two reads in flight at once, in the order the test chooses.
let heldList: Array<() => void> | null = null;
function route(cmd: string, args?: unknown): Promise<unknown> {
  if (cmd === 'list_history') {
    const filter = (args as { filter?: string } | undefined)?.filter;
    const fails = () => failList || (failFilter !== null && (filter ?? '') === failFilter);
    const shown = filter
      ? history.filter((h) => `${h.query}\n${answers[h.id] ?? h.preview}`.toLowerCase().includes(filter.toLowerCase()))
      : history;
    const reply = () => shown.map((h) => ({ ...h }));
    if (heldList) {
      return new Promise((resolve, reject) => heldList!.push(() => {
        if (fails()) reject(new Error('list_history failed'));
        else resolve(reply());
      }));
    }
    if (fails()) return Promise.reject(new Error('list_history failed'));
    return Promise.resolve(reply());
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
function rowPart(id: number, part: 'recents-query' | 'recents-preview') {
  return screen.getByTestId(`recents-row-${id}`).querySelector(`[data-testid="${part}"]`)!.textContent;
}
function groupOf(id: number): string | null {
  return screen.getByTestId(`recents-row-${id}`).closest('[data-testid^="recents-group-"]')!.getAttribute('data-testid');
}

// The clock is pinned to LOCAL noon on 2026-10-21, and every entry below is
// built from local date parts: a group's edge is local midnight, so a literal
// UTC stamp would only be right in the zone its author happened to sit in.
const NOW_MS = new Date(2026, 9, 21, 12, 0, 0).getTime();
const at = (daysAgo: number, hour = 12) =>
  Math.round(new Date(2026, 9, 21 - daysAgo, hour).getTime() / 1000);
function entry(id: number, query: string, daysAgo: number): HistoryEntry {
  return { id, query, preview: `${query} answered`, askedAt: at(daysAgo) };
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(route);
  history = [];
  answers = {};
  heldList = null;
  failList = false;
  failFilter = null;
  failDelete = false;
  setLocale('en');
});

// `locale` is a module-level store shared by every test in this file, and a
// pinned clock outlives a failed assertion: both are restored unconditionally.
afterEach(() => {
  setLocale('en');
  vi.restoreAllMocks();
});

// A row is the question with ONE clipped line of its answer under it. When it
// was asked is the group's business now, not the row's.
test('a row draws the question and a preview of the answer, newest first', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-1')).toBeTruthy();
  // The order is the order `list_history` sent, newest first: the list does not sort.
  expect(screen.getAllByTestId(/^recents-row-/).map((el) => el.dataset.testid))
    .toEqual(['recents-row-2', 'recents-row-1']);
  expect([
    rowPart(2, 'recents-query'),
    rowPart(2, 'recents-preview'),
    rowPart(1, 'recents-query'),
    rowPart(1, 'recents-preview'),
  ]).toEqual([
    'when does the lease renew',
    'The renewal date is in the signed copy.',
    'invoice payment terms',
    'Payment is due thirty days after the invoice date.',
  ]);
  // Two lines inside the one open button, and no relative time anywhere.
  expect(within(screen.getByTestId('recents-row-2')).getByTestId('recents-open-2').children).toHaveLength(2);
  expect(screen.queryByText(/minutes ago/)).toBeNull();
});

// The backend sends `preview: ""` when an answer had nothing to cut from it.
test('a row whose answer left nothing to preview draws one line', async () => {
  mockHistory([{ id: 1, query: 'a question', preview: '', askedAt: at(0) }]);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  const row = await screen.findByTestId('recents-row-1');
  expect(within(row).queryByTestId('recents-preview')).toBeNull();
  expect(within(row).getByTestId('recents-open-1').children).toHaveLength(1);
});

// Every entry keeps its place: the buckets are the local CALENDAR day, and the
// oldest rows still get a group of their own rather than vanishing.
test('a row lands under the group its day belongs to', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW_MS);
  // id 5 is 00:00 today, id 4 an hour before it — yesterday's date.
  const list = [
    { id: 5, query: 'today question', preview: 'p5', askedAt: at(0, 0) },
    { id: 4, query: 'late question', preview: 'p4', askedAt: at(1, 23) },
    entry(3, 'yesterday question', 1),
    entry(2, 'week question', 7),
    entry(1, 'older question', 8),
  ];
  mockHistory(list);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-5')).toBeTruthy();
  expect(screen.getAllByTestId(/^recents-group-[a-z]+$/).map((el) => el.dataset.testid))
    .toEqual(['recents-group-today', 'recents-group-yesterday', 'recents-group-week', 'recents-group-earlier']);
  expect([groupOf(5), groupOf(4), groupOf(3), groupOf(2), groupOf(1)]).toEqual([
    'recents-group-today', 'recents-group-yesterday', 'recents-group-yesterday',
    'recents-group-week', 'recents-group-earlier',
  ]);
  expect(within(screen.getByTestId('recents-group-earlier')).getByRole('heading').textContent).toBe('Earlier');
});

// A bucket nobody falls into draws no heading.
test('an empty group draws no heading', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW_MS);
  mockHistory([entry(1, 'a question', 0)]);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-group-today')).toBeTruthy();
  expect(screen.queryByTestId('recents-group-yesterday')).toBeNull();
  expect(screen.queryByTestId('recents-group-week')).toBeNull();
  expect(screen.queryByTestId('recents-group-earlier')).toBeNull();
});

test('the group headings follow a live language switch', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW_MS);
  mockHistory([entry(1, 'a question', 0), entry(2, 'an older question', 20)]);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  const heads = () => screen.getAllByRole('heading', { level: 3 }).map((h) => h.textContent);
  expect(await screen.findByTestId('recents-group-today')).toBeTruthy();
  expect(heads()).toEqual(['Today', 'Earlier']);

  setLocale('uk');
  await tick();
  expect(heads()).toEqual(['Сьогодні', 'Раніше']);

  setLocale('en'); // the switch back is part of the claim, not the cleanup
  await tick();
  expect(heads()).toEqual(['Today', 'Earlier']);
});

// The panel's own header: the name, and the second way to start a conversation
// (the centre header's «New» stays). It sits outside `.recents-body`, so fifty
// rows cannot scroll it away.
test('the header names the panel and offers a new conversation', async () => {
  mockHistory(historyTwoQuestions);
  const onNew = vi.fn();
  render(Recents, { activeId: null, gen: 0, onOpen: () => {}, onNew });

  const title = await screen.findByTestId('recents-title');
  await screen.findByTestId('recents-row-2');
  expect(title.textContent).toBe('Conversations');
  expect(document.querySelector('.recents-body')!.contains(title)).toBe(false);

  const compose = screen.getByRole('button', { name: 'New conversation' });
  expect(compose).toBe(screen.getByTestId('recents-new'));
  await fireEvent.click(compose);
  expect(onNew).toHaveBeenCalledTimes(1);

  setLocale('uk');
  await tick();
  expect(title.textContent).toBe('Бесіди');
  expect(screen.getByRole('button', { name: 'Нова бесіда' })).toBe(compose);
});

test('the compose button is disabled while a question is in flight', async () => {
  const onNew = vi.fn();
  const { rerender } = render(Recents, { activeId: null, gen: 0, onOpen: () => {}, onNew });
  const compose = await screen.findByTestId('recents-new');
  expect(compose.hasAttribute('disabled')).toBe(false);

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, onNew, newDisabled: true });
  expect(compose.hasAttribute('disabled')).toBe(true);
  expect(onNew).not.toHaveBeenCalled();
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

test('two questions draw two rows, each carrying its own question', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  expect(await screen.findByTestId('recents-row-1')).toBeTruthy();
  expect(screen.getAllByTestId('recents-query').map((el) => el.textContent))
    .toEqual(['when does the lease renew', 'invoice payment terms']);
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
  mockHistory([{ id: 3, query: 'a question just asked', preview: '', askedAt: at(0) }, ...historyTwoQuestions]);
  await rerender({ activeId: null, gen: 1, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-3')).toBeTruthy();
  expect(historyCalls()).toHaveLength(3);
});

// An open that failed was clicked here, and it is said here rather than under a
// composer the person did not touch.
// The list is read for `gen` and for nothing else: a parent that re-sets its
// other props (a new active row) must not send another read.
test('a changed active row does not read the list again', async () => {
  mockHistory(historyTwoQuestions);
  const { rerender } = render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
  expect(historyCalls()).toHaveLength(1);

  await rerender({ activeId: 2, gen: 0, onOpen: () => {} });
  await tick();
  expect(historyCalls()).toHaveLength(1);
});

test('a failed open says so beside the list', async () => {
  const { rerender } = render(Recents, { activeId: null, gen: 0, onOpen: () => {}, error: null });
  expect(screen.queryByTestId('recents-error')).toBeNull();

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: 'historyUnreadable' });
  expect((await screen.findByTestId('recents-error')).textContent).toBe('This entry cannot be opened.');

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: 'historyFailed' });
  expect((await screen.findByTestId('recents-error')).textContent).toBe('This conversation could not be opened.');

  await rerender({ activeId: null, gen: 0, onOpen: () => {}, error: null });
  await tick();
  expect(screen.queryByTestId('recents-error')).toBeNull();
});

// The filter, from §4's mockup: one rounded field under the panel's name, named
// for a screen reader (the placeholder is not a name), and pinned with the
// header so it cannot scroll away from the list it filters.
const searchBox = () => screen.getByRole('searchbox', { name: 'Search conversations' });

test('a search field sits under the panel name and names itself from the catalog', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });

  const field = await screen.findByRole('searchbox', { name: 'Search conversations' });
  expect(field).toBe(screen.getByTestId('recents-filter'));
  expect(field.getAttribute('type')).toBe('search');
  expect(field.getAttribute('placeholder')).toBe('Search');
  // Outside the body that scrolls, beside the pinned header.
  expect(document.querySelector('.recents-body')!.contains(field)).toBe(false);
  // Nothing typed yet: the read carries no filter at all.
  await waitFor(() => expect(historyCalls()).toHaveLength(1));
  expect(historyCalls()[0]).toEqual(['list_history', { filter: undefined }]);

  setLocale('uk');
  await tick();
  const ukField = screen.getByRole('searchbox', { name: 'Пошук у бесідах' });
  expect(ukField).toBe(field);
  expect(ukField.getAttribute('placeholder')).toBe('Пошук');
});

// Typing is the filter: it travels trimmed, and the rows are whatever the
// backend answered — this component does no matching of its own.
test('typing sends the word to the backend, trimmed, and draws what it answered', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();

  await fireEvent.input(searchBox(), { target: { value: '  invoice  ' } });

  await waitFor(() => expect(historyCalls().at(-1)).toEqual(['list_history', { filter: 'invoice' }]));
  await waitFor(() => expect(screen.queryByTestId('recents-row-2')).toBeNull());
  expect(screen.getByTestId('recents-row-1')).toBeTruthy();

  // Clearing reads the unfiltered list again.
  await fireEvent.input(searchBox(), { target: { value: '' } });
  await waitFor(() => expect(historyCalls().at(-1)).toEqual(['list_history', { filter: undefined }]));
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
});

// The stored answer is longer than the drawn line, which is why the match is
// the backend's: a word that only the full answer holds still finds its row.
test('a word only the stored answer holds finds the row, not just the preview', async () => {
  mockHistory(historyTwoQuestions);
  answers = { 2: 'The deposit is held by the letting agent.' };
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
  expect(rowPart(2, 'recents-preview')).not.toContain('deposit');

  await fireEvent.input(searchBox(), { target: { value: 'deposit' } });

  await waitFor(() => expect(screen.queryByTestId('recents-row-1')).toBeNull());
  expect(screen.getByTestId('recents-row-2')).toBeTruthy();
});

// An empty filtered list is not an empty history: the old sentence would say
// there are no questions while they sit one word away.
test('a filtered list with no match says so, and the unfiltered one keeps its own words', async () => {
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect((await screen.findByTestId('recents-empty')).textContent).toBe('Your questions will appear here');

  await fireEvent.input(searchBox(), { target: { value: 'nothing matches this' } });

  expect((await screen.findByTestId('recents-nothing-found')).textContent).toBe('Nothing found');
  expect(screen.queryByTestId('recents-empty')).toBeNull();

  await fireEvent.input(searchBox(), { target: { value: '' } });
  expect((await screen.findByTestId('recents-empty')).textContent).toBe('Your questions will appear here');
});

// The sentence belongs to the result ON SCREEN, not to the live field: clearing
// the box after a filtered-empty result must not flash "your questions will
// appear here" while the previous, empty result is still the one drawn.
test('the empty sentence follows the result on screen, not the live field', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();

  await fireEvent.input(searchBox(), { target: { value: 'nothing matches this' } });
  expect(await screen.findByTestId('recents-nothing-found')).toBeTruthy();

  const parked: Array<() => void> = [];
  heldList = parked;
  await fireEvent.input(searchBox(), { target: { value: '' } });
  await waitFor(() => expect(parked).toHaveLength(1)); // the unfiltered read, held

  // Still the filtered result: still its sentence, and not the empty-history one.
  expect(screen.getByTestId('recents-nothing-found')).toBeTruthy();
  expect(screen.queryByTestId('recents-empty')).toBeNull();

  heldList = null;
  parked.forEach((release) => release());
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();
  expect(screen.queryByTestId('recents-nothing-found')).toBeNull();
});

// The reviewed race: the mount read is slow, the filtered read answers first
// and paints, and then the mount read fails. A stale failure knows nothing
// about the list on screen, so it must not replace it with an error.
test('a stale failed read does not hide the newer list', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockHistory(historyTwoQuestions);
  const parked: Array<() => void> = [];
  heldList = parked;
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  await waitFor(() => expect(parked).toHaveLength(1)); // the mount read, held
  failFilter = ''; // ...and it will reject

  await fireEvent.input(searchBox(), { target: { value: 'invoice' } });
  await waitFor(() => expect(parked).toHaveLength(2));

  heldList = null;
  parked[1](); // the newer read lands first
  await waitFor(() => expect(screen.getByTestId('recents-row-1')).toBeTruthy());
  parked[0](); // the stale read rejects afterwards
  await waitFor(() => expect(err).toHaveBeenCalled());

  expect(screen.getByTestId('recents-row-1')).toBeTruthy();
  expect(screen.queryByTestId('recents-failed')).toBeNull();
});

// The same rule the other way: a stale SUCCESS must not paint rows for its own
// filter over a newer read that has just failed.
test('a stale successful read does not paint under a newer failure', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockHistory(historyTwoQuestions);
  const parked: Array<() => void> = [];
  heldList = parked;
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  await waitFor(() => expect(parked).toHaveLength(1)); // the mount read, held
  failFilter = 'invoice';

  await fireEvent.input(searchBox(), { target: { value: 'invoice' } });
  await waitFor(() => expect(parked).toHaveLength(2));

  heldList = null;
  parked[1](); // the newer read fails
  await waitFor(() => expect(screen.getByTestId('recents-failed')).toBeTruthy());
  parked[0](); // the stale read succeeds afterwards
  await waitFor(() => expect(err).toHaveBeenCalled());

  expect(screen.getByTestId('recents-failed')).toBeTruthy();
  expect(screen.queryByTestId('recents-row-2')).toBeNull();
});

// A read that fails for a NEW filter has no rows it can honestly draw: the ones
// on screen answer a different question. It says so instead.
test('a read that fails for a new filter says so and hides the old rows', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();

  failList = true;
  await fireEvent.input(searchBox(), { target: { value: 'zzz' } });

  expect(await screen.findByTestId('recents-failed')).toBeTruthy();
  expect(screen.queryByTestId('recents-row-2')).toBeNull();
  expect(err).toHaveBeenCalled();
});

// The old rule still holds where it was written: a refresh that fails under the
// SAME filter keeps the list, because those rows still answer that question.
test('a refresh that fails under the same filter keeps the rows', async () => {
  const err = vi.spyOn(console, 'error').mockImplementation(() => {});
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();

  failList = true;
  await fireEvent.focus(window);
  await waitFor(() => expect(historyCalls()).toHaveLength(2));
  await waitFor(() => expect(err).toHaveBeenCalled());

  expect(screen.getByTestId('recents-row-2')).toBeTruthy();
  expect(screen.queryByTestId('recents-failed')).toBeNull();
});

// A delete re-reads the list, and that read keeps the filter the person is
// looking through — losing it would repopulate the panel with rows they had
// just filtered away.
test('deleting under a filter re-reads with that filter', async () => {
  mockHistory(historyTwoQuestions);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-2')).toBeTruthy();

  await fireEvent.input(searchBox(), { target: { value: 'lease' } });
  await waitFor(() => expect(screen.queryByTestId('recents-row-1')).toBeNull());

  await fireEvent.click(screen.getByTestId('recents-remove-2'));
  await waitFor(() => expect(historyCalls().at(-1)).toEqual(['list_history', { filter: 'lease' }]));
});

// The groups are the list's own grouping, over whatever the filter returned.
test('a filtered list is still grouped by day', async () => {
  vi.spyOn(Date, 'now').mockReturnValue(NOW_MS);
  mockHistory([
    { id: 2, query: 'invoice today', preview: 'p2', askedAt: at(0) },
    { id: 1, query: 'invoice yesterday', preview: 'p1', askedAt: at(1) },
    { id: 3, query: 'something else', preview: 'p3', askedAt: at(0) },
  ]);
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  expect(await screen.findByTestId('recents-row-3')).toBeTruthy();

  await fireEvent.input(searchBox(), { target: { value: 'invoice' } });

  await waitFor(() => expect(screen.queryByTestId('recents-row-3')).toBeNull());
  expect(screen.getAllByTestId(/^recents-group-[a-z]+$/).map((el) => el.dataset.testid))
    .toEqual(['recents-group-today', 'recents-group-yesterday']);
  expect([groupOf(2), groupOf(1)]).toEqual(['recents-group-today', 'recents-group-yesterday']);
});

// Two reads in flight, the newer one answering first: the mount's slower,
// unfiltered reply must not repaint the list the filter already narrowed.
test('a slower unfiltered reply cannot overwrite the filtered list', async () => {
  mockHistory(historyTwoQuestions);
  const parked: Array<() => void> = [];
  heldList = parked;
  render(Recents, { activeId: null, gen: 0, onOpen: () => {} });
  await waitFor(() => expect(parked).toHaveLength(1)); // the mount's read, held

  heldList = null; // every later read answers at once
  await fireEvent.input(searchBox(), { target: { value: 'invoice' } });
  await waitFor(() => expect(screen.getByTestId('recents-row-1')).toBeTruthy());
  expect(screen.queryByTestId('recents-row-2')).toBeNull();

  parked[0](); // the mount's unfiltered reply lands last
  await tick();
  expect(screen.queryByTestId('recents-row-2')).toBeNull();
  expect(screen.getByTestId('recents-row-1')).toBeTruthy();
});
