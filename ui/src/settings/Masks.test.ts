import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Masks from './Masks.svelte';
import { setLocale, t } from '../i18n';

// Mocked as the typed wrappers, in the shape `Folders.test.ts:13-34` uses:
// what this file is about is the screen, and `ipc.test.ts` owns the wire names
// and argument spellings each of these sends.
const listMasks = vi.fn();
const maskPreview = vi.fn();
const addMask = vi.fn();
const removeMask = vi.fn();
vi.mock('../lib/ipc', async (real) => ({
  ...(await real<Record<string, unknown>>()),
  listMasks: (...a: unknown[]) => listMasks(...a),
  maskPreview: (...a: unknown[]) => maskPreview(...a),
  addMask: (...a: unknown[]) => addMask(...a),
  removeMask: (...a: unknown[]) => removeMask(...a),
}));

// The editor computes no counts of its own (Task 10, review round 1 P1), and
// this is the guard on that: `mask_preview` is the ONLY thing that may answer
// "how much goes", so nothing in this component may reach the wire on its own.
// The raw `invoke` is mocked here for the reason `Folders.test.ts:51-55` gives
// — a wrapper renamed in a later task must not quietly retire the guard — and
// the shared `afterEach` below asserts it was never called, for every test in
// this file rather than one.
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {},
}));

beforeEach(() => {
  listMasks.mockReset();
  maskPreview.mockReset();
  addMask.mockReset();
  removeMask.mockReset();
  invoke.mockReset();
  listMasks.mockResolvedValue([]);
  // The ordinary outcome, so a test about something else does not have to
  // restate it. `add_mask` answers which of two things happened and every
  // press has to read that answer, so a bare `vi.fn()` returning `undefined`
  // would fail every add flow for a reason unrelated to its subject.
  addMask.mockResolvedValue({ kind: 'stored' });
});
afterEach(() => {
  expect(invoke).not.toHaveBeenCalled();
  cleanup();
  setLocale('en'); // the store outlives the component; leave it as found
});

// Every text node under `el`, in document order, joined by a single space —
// what a person reads, in the order they read it. NOT `textContent`, for the
// reason `Folders.test.ts:375-378` gives: that concatenates two neighbouring
// rows into one word wherever the markup leaves no whitespace between them.
function visibleText(el: HTMLElement): string {
  const walker = el.ownerDocument.createTreeWalker(el, 4 /* SHOW_TEXT */);
  const parts: string[] = [];
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const text = (node.textContent ?? '').replace(/\s+/g, ' ').trim();
    if (text !== '') parts.push(text);
  }
  return parts.join(' ');
}

async function mount(masks: string[] = []) {
  setLocale('en'); // seed, do not inherit: a sibling switching the language must not decide this test
  listMasks.mockResolvedValue(masks);
  const rendered = render(Masks);
  await waitFor(() => expect(listMasks).toHaveBeenCalled());
  return rendered;
}

// Types a mask and presses the add control. The two are one move: a draft that
// never reaches the field is a draft the button reads as blank.
async function type(mask: string) {
  await fireEvent.input(screen.getByRole('textbox'), { target: { value: mask } });
}
const addButton = () => screen.getByRole('button', { name: 'Add a mask' });
// Named by the mask, never by the word on the face: `Confirm` and `Cancel` are
// the same two words for every question this section can ask, and only the
// accessible name says which mask the press is about. The visible word is
// asserted separately, in the test that presses one.
const confirmAdd = (mask: string) => screen.getByRole('button', { name: `Confirm adding the mask ${mask}` });
const confirmRemove = (mask: string) => screen.getByRole('button', { name: `Confirm removing the mask ${mask}` });
const cancelFor = (mask: string) => screen.getByRole('button', { name: `Leave ${mask} as it is` });

test('the list renders every stored mask, not the first of them', async () => {
  // Two, and different from each other: a one-element fixture is satisfied by a
  // component that renders `masks[0]` and stops.
  const { container } = await mount(['*.pdf', 'Копія*']);

  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());
  expect(screen.getByText('Копія*')).toBeTruthy();
  expect(visibleText(container)).not.toContain('No file mask has been added yet.');
});

test('an empty list says so rather than showing a heading with nothing under it', async () => {
  await mount([]);

  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
  // Both directions: the empty sentence is here AND no remove control is, which
  // is what a list rendered from a stale array would still show.
  expect(screen.queryByRole('button', { name: /^Remove the mask/ })).toBeNull();
});

test('adding asks the preview first and shows both its numbers before anything is stored', async () => {
  // 🔴 `paths` and `documents` deliberately differ: a fixture where they are
  // equal cannot tell the two numbers apart, and swapping them in the markup
  // would stay green through it.
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  expect(maskPreview).toHaveBeenCalledWith('*.pdf');
  // Nothing stored while the question stands — asserted on the command, not on
  // the list looking unchanged.
  expect(addMask).not.toHaveBeenCalled();

  const cost = visibleText(screen.getByTestId('mask-confirm-cost'));
  expect(cost).toBe(
    'As of now, at least 4 files already indexed match this mask, and 2 documents stop being'
    + ' findable: no other path names them. The next scan of each folder can remove more than'
    + ' that: files that never finished indexing are not counted here.',
  );
});

// 🔴 The wait is not instant and must not be silent. `mask_preview` holds the
// index mutex across a scan of every indexed path of every root, so a press
// made while a walk is running waits on that lock — and a screen that answers a
// press with nothing is a screen that invites a second press.
test('the press says it is checking, and the numbers replace that sentence rather than joining it', async () => {
  let release: (v: { paths: number; documents: number }) => void = () => {};
  maskPreview.mockReturnValue(new Promise<{ paths: number; documents: number }>((resolve) => { release = resolve; }));
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByText('Checking what this mask removes…')).toBeTruthy());
  // Both directions: the wait is announced AND no cost sentence is on screen
  // yet, because no number has arrived to put in one.
  expect(screen.queryByTestId('mask-confirm-cost')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Confirm adding the mask *.pdf' })).toBeNull();

  release({ paths: 4, documents: 2 });
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  expect(screen.queryByText('Checking what this mask removes…')).toBeNull();
});

// Task 11 fix round 1, F1. A late `mask_preview` reply has no generation guard
// of its own, although the list read three lines below (`reads`) has exactly
// this one. The ordinary route: type a mask, press Add, get impatient before
// the mutex-bound preview answers, press Remove on a stored mask, and read
// ITS question — the control under the finger has to still mean "remove",
// not silently become "confirm adding" once the late reply lands.
test('a late add-preview must not replace a standing removal question', async () => {
  let releaseAdd: (v: { paths: number; documents: number }) => void = () => {};
  maskPreview.mockReturnValue(new Promise((resolve) => { releaseAdd = resolve; }));
  await mount(['*.tmp']);
  await waitFor(() => expect(screen.getByText('*.tmp')).toBeTruthy());

  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByText('Checking what this mask removes…')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.tmp' }));
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  expect(visibleText(screen.getByTestId('mask-confirm'))).toContain('Remove the mask *.tmp?');
  expect(confirmRemove('*.tmp')).toBeTruthy();

  // The add preview answers only now, after the question on screen changed.
  // A real timer tick, not `Promise.resolve()`: Svelte 5's effect flush after
  // this promise's continuation needs more than the microtask queue alone.
  releaseAdd({ paths: 4, documents: 2 });
  await new Promise((r) => setTimeout(r, 0));

  // Both directions: the removal question is still the one standing, and the
  // late reply did not put the add question up in its place.
  expect(visibleText(screen.getByTestId('mask-confirm'))).toContain('Remove the mask *.tmp?');
  expect(screen.queryByText('Add the mask *.pdf?')).toBeNull();
  expect(confirmRemove('*.tmp')).toBeTruthy();
});

// Task 11 fix round 1, F1, second route to the same missing guard: nothing
// disabled Add while a check was on the wire, so two `mask_preview` calls could
// queue on the index mutex with no order guarantee between them. Mirrors
// `the older of two overlapping list reads writes nothing` below, for `reads`.
test('the older of two overlapping mask previews writes nothing', async () => {
  let releaseFirst: (v: { paths: number; documents: number }) => void = () => {};
  let releaseSecond: (v: { paths: number; documents: number }) => void = () => {};
  maskPreview
    .mockReturnValueOnce(new Promise((resolve) => { releaseFirst = resolve; }))
    .mockReturnValueOnce(new Promise((resolve) => { releaseSecond = resolve; }));
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());
  await type('*.tmp');
  await fireEvent.click(addButton());
  expect(maskPreview).toHaveBeenCalledTimes(2);

  // The newer (second) preview answers first.
  releaseSecond({ paths: 1, documents: 1 });
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  expect(visibleText(screen.getByTestId('mask-confirm'))).toContain('Add the mask *.tmp?');

  // The older (first) preview answers last and must not overwrite it.
  releaseFirst({ paths: 9, documents: 9 });
  await new Promise((r) => setTimeout(r, 0));

  expect(visibleText(screen.getByTestId('mask-confirm'))).toContain('Add the mask *.tmp?');
  expect(screen.queryByText('Add the mask *.pdf?')).toBeNull();
  expect(confirmAdd('*.tmp')).toBeTruthy();
});

// 🔴 `mask_preview.paths` counts `status = 'indexed'` rows only, while the
// walk's reconcile set is status-agnostic — so the walk's `removed` can exceed
// it, and "4 files will be removed" followed by `removed: 6` reads as a lie.
// The sentence states a floor and names the remainder instead.
test('the file count is stated as a floor, and the sentence names what it leaves out', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visibleText(screen.getByTestId('mask-confirm-cost'));
  expect(cost).toContain('at least 4 files');
  expect(cost).toContain('can remove more than that');
  expect(cost).toContain('files that never finished indexing are not counted here');
});

// 🔴 The state the two numbers exist to tell apart: every matched path has a
// second copy elsewhere, so paths go and no document stops being findable. A
// person told "0 documents" beside a folder they know holds files needs the
// first number to make sense of it.
test('zero documents beside a non-zero file count says the documents stay findable', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 0 });
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visibleText(screen.getByTestId('mask-confirm-cost'));
  expect(cost).toContain('at least 4 files');
  expect(cost).toContain('no document stops being findable — each one is also indexed under another path');
  // The direction a swap of the two numbers would produce, and the one an
  // overstated disclosure reads as.
  expect(cost).not.toContain('4 documents');
  expect(cost).not.toContain('at least 0 files');
});

// A preview of two zeros is not "this rule would remove nothing": it is "the
// indexed set holds nothing that matches, today". The next scan can still take
// files that never finished indexing, so the question is asked anyway — there
// is no `paths === 0` shortcut past it.
test('a preview of zero asks the question anyway, in a sentence of its own', async () => {
  maskPreview.mockResolvedValue({ paths: 0, documents: 0 });
  await mount([]);

  await type('*.nothing');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  const cost = visibleText(screen.getByTestId('mask-confirm-cost'));
  expect(cost).toBe(
    'As of now, no file that is already indexed matches this mask. The next scan of each folder'
    + ' can still remove files: those that never finished indexing are not counted here.',
  );
  // The zero arm of the shared sentence would say this, and it has nobody to
  // say it about when no path matched at all.
  expect(cost).not.toContain('also indexed under another path');
  expect(addMask).not.toHaveBeenCalled();
});

test('confirming stores the mask as typed and re-reads the list', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  addMask.mockResolvedValue({ kind: 'stored' });
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());

  // The second read answers differently from the first: a list that "happened
  // to be right" cannot tell a re-read from a stale array left on screen.
  listMasks.mockResolvedValue(['*.pdf']);
  // The face says Confirm and the accessible name says what is being confirmed:
  // both are asserted, because a screen reader and a pair of eyes read
  // different halves of this control.
  expect(confirmAdd('*.pdf').textContent).toBe('Confirm');
  await fireEvent.click(confirmAdd('*.pdf'));

  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());
  expect(addMask).toHaveBeenCalledWith('*.pdf');
  expect(listMasks).toHaveBeenCalledTimes(2);
  // The field is emptied, so the next press cannot re-add what was just added.
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('');
});

test('cancelling stores nothing, and the question goes with the press', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());

  expect(cancelFor('*.pdf').textContent).toBe('Cancel');
  await fireEvent.click(cancelFor('*.pdf'));

  // Asserted on the command, never on the list looking unchanged: an empty
  // list looks unchanged whatever was stored.
  expect(addMask).not.toHaveBeenCalled();
  expect(screen.queryByTestId('mask-confirm')).toBeNull();
  // And nothing was re-read either: a cancel that quietly refreshed would hide
  // a store behind the same green assertion above.
  expect(listMasks).toHaveBeenCalledTimes(1);
});

// 🔴 A rejection travels as a sentence, never as a kind. The backend's own
// words are shown verbatim; what the editor adds around them is the mask AS
// TYPED, because `RulesError::InvalidMask`'s `reason` quotes the FOLDED
// pattern — someone who typed `[A-_]x.txt` reads about `[a-_]x.txt`.
test('a refused mask shows the backend sentence verbatim, names the mask as typed, and stores nothing', async () => {
  const sentence =
    'file mask "[A-_]x.txt" could not be compiled: error parsing glob \'[a-_]x.txt\':'
    + ' unclosed character class; missing \']\'';
  maskPreview.mockRejectedValue(new Error(sentence));
  await mount([]);

  await type('[A-_]x.txt');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  // Verbatim, character for character: nothing rewritten, nothing summarised.
  expect(screen.getByTestId('mask-refused-reason').textContent).toBe(sentence);
  // The frame around it, and the case note that explains the second spelling
  // inside the sentence.
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe('The mask [A-_]x.txt was not added. This is what the check answered:');
  expect(screen.getByText(
    'The answer above can quote your mask in a different letter case than the one you typed:'
    + ' masks are compared with letter case ignored.',
  )).toBeTruthy();

  expect(addMask).not.toHaveBeenCalled();
  expect(screen.queryByTestId('mask-confirm')).toBeNull();
});

// The rejection that arrives AFTER the preview said yes — the index closing
// between the two calls, for one. The catch on the store is a second catch, and
// a component with only the first would swallow this whole.
test('a refusal from the store itself is shown too, and the list is read again', async () => {
  maskPreview.mockResolvedValue({ paths: 1, documents: 1 });
  addMask.mockRejectedValue(new Error('the index is not open'));
  await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.pdf'));

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(screen.getByTestId('mask-refused-reason').textContent).toBe('the index is not open');
  // Task 11 fix round 1, F4. The check already passed — that is why a question
  // was on screen to confirm — so this failure is the STORE's, not the
  // check's, and the frame must say so rather than reusing the check's words.
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe('The mask *.pdf was not stored. This is what the index answered:');
  // The case note explains a folded pattern quoted inside a COMPILE refusal;
  // the check already passed here, so nothing folded anything.
  expect(screen.queryByText(
    'The answer above can quote your mask in a different letter case than the one you typed:'
    + ' masks are compared with letter case ignored.',
  )).toBeNull();
  // The screen after a refusal is a fresh read, not the array from before it.
  expect(listMasks).toHaveBeenCalledTimes(2);
});

// Task 11 fix round 1, F3. The `of: 'add' | 'remove'` decision behind
// `refusal.heading` and `refusal.note` (`Masks.svelte:183-193`) had no test on
// its removal half: a `remove_mask` that throws is reachable — the same
// failure the add path's own test above simulates with "the index is not
// open" — and nothing here forced the heading to say "removed" rather than
// "added", or the case note to stay off a path that folds nothing.
test('a refusal from removing a mask names the removal, not the add, and carries no case note', async () => {
  removeMask.mockRejectedValue(new Error('the index is not open'));
  await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.pdf' }));
  await fireEvent.click(confirmRemove('*.pdf'));

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(screen.getByTestId('mask-refused-reason').textContent).toBe('the index is not open');
  // Verbatim, and the class this repository names: a screen must not state
  // what the data contradicts. Nothing was added, so "was not added" here
  // would be exactly that.
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe('The mask *.pdf was not removed. This is what the index answered:');
  // The case note explains a FOLDED pattern quoted inside a compile refusal;
  // nothing on the removal path folds anything, so it must not appear here.
  expect(screen.queryByText(
    'The answer above can quote your mask in a different letter case than the one you typed:'
    + ' masks are compared with letter case ignored.',
  )).toBeNull();
  // The mask is still stored: the failed removal did not silently succeed.
  expect(screen.getByText('*.pdf')).toBeTruthy();
});

// 🔴 Task 11 fix round 2, F2/F3. The state the live run found: with `*.pdf`
// stored, typing `*.PDF` was accepted, appeared as a second row, and the screen
// said nothing — two lines under the section's own sentence that the two are
// one rule. The harm is the removal that follows: `*.pdf` removed gives no
// files back, because `*.PDF` is still holding them.
//
// 🔴 The list is asserted as well as the sentence, and that is what makes this
// a test rather than an echo. Say what it guards precisely, because an earlier
// wording claimed more than it can see: `addMask` here is a mock and the list
// comes from the `listMasks` mock, so whether a ROW WAS STORED is not
// observable from this file at all — that guarantee lives in
// `src-tauri/tests/commands.rs`. What the count assertion below catches is the
// component appending the draft to the list optimistically, on a press that
// stored nothing.
test('a rule that is already stored under another spelling is not added, and the sentence names the stored spelling', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 2 });
  addMask.mockResolvedValue({ kind: 'alreadyStored', stored: '*.pdf' });
  await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  await type('*.PDF');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.PDF'));

  await waitFor(() => expect(screen.getByTestId('mask-already-stored')).toBeTruthy());
  // The stored spelling by name, not "you already have this": the person typed
  // `*.PDF` and has to be able to find `*.pdf` in the list above.
  expect(screen.getByTestId('mask-already-stored').textContent)
    .toBe('You already have this rule — it is stored as *.pdf. Nothing was added.');
  // Exactly one row, by count and not by "the one we know about is there": a
  // second row would leave the first assertion green.
  expect(screen.getAllByRole('button', { name: /^Remove the mask/ }).length).toBe(1);
  expect(screen.getByRole('button', { name: 'Remove the mask *.pdf' })).toBeTruthy();
  expect(screen.queryByText('*.PDF')).toBeNull();

  // The other direction, on a mask that really is new: the sentence must not
  // stand on a press it does not describe, and the add must still happen.
  cleanup();
  addMask.mockResolvedValue({ kind: 'stored' });
  await mount([]);
  listMasks.mockResolvedValue(['*.tmp']);

  await type('*.tmp');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.tmp'));

  await waitFor(() => expect(screen.getByText('*.tmp')).toBeTruthy());
  expect(addMask).toHaveBeenLastCalledWith('*.tmp');
  expect(screen.queryByTestId('mask-already-stored')).toBeNull();
});

// Task 11 fix round 2, F5. The caveat rendered under BOTH refusals in the live
// run — `sub/*.txt` and `!notes.txt` — where the answer echoed the mask byte
// for byte and there was nothing for it to explain. It is needed for the real
// case and only there.
//
// One test, both directions, because a caveat that never renders and a caveat
// that always renders are the two failures and a one-sided assertion is
// satisfied by one of them.
test('the case note stands only where the answer really spells the mask differently', async () => {
  const CASE_NOTE =
    'The answer above can quote your mask in a different letter case than the one you typed:'
    + ' masks are compared with letter case ignored.';

  // The answer quotes `[a-_]x.txt` — the FOLDED pattern `globset` was handed,
  // which the person never typed. This is what the note is for.
  maskPreview.mockRejectedValue(new Error(
    'file mask "[A-_]x.txt" could not be compiled: error parsing glob \'[a-_]x.txt\':'
    + ' unclosed character class; missing \']\'',
  ));
  await mount([]);
  await type('[A-_]x.txt');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(screen.getByText(CASE_NOTE)).toBeTruthy();

  // The same refusal frame, from the same command, over an answer that spells
  // the mask exactly as it was typed. Live: `file mask "sub/*.txt" cannot
  // contain `/` — …`, and the caveat stood under it explaining a change that
  // had not happened.
  cleanup();
  maskPreview.mockRejectedValue(new Error(
    'file mask "sub/*.txt" cannot contain `/` — a mask names a file, and a folder is'
    + ' excluded with an exclusion rule instead',
  ));
  await mount([]);
  await type('sub/*.txt');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  // The frame and the shell's sentence are still there — this is not a screen
  // that lost its refusal, it is one that lost a caveat it did not need.
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe('The mask sub/*.txt was not added. This is what the check answered:');
  expect(screen.queryByText(CASE_NOTE)).toBeNull();

  // 🔴 The state the two fixtures above do not build, and the reason this
  // block exists. Both of them move two variables at once: `[A-_]x.txt` has an
  // uppercase letter AND a respelling, `sub/*.txt` has neither — so a locator
  // that ignored the mask entirely and answered "does this sentence contain any
  // uppercase character" satisfies both. Measured by independent review: with
  // the whole function replaced by `void mask; return answer.toLowerCase() !==
  // answer;` the entire UI suite stayed green, 570 of 570.
  //
  // This is that mutant's fixture: an UPPERCASE mask the answer quotes exactly
  // as it was typed. The caveat must stay off, and only a locator that actually
  // compares the mask can keep it off.
  cleanup();
  maskPreview.mockRejectedValue(new Error(
    'file mask "SUB/*.txt" cannot contain `/` — a mask names a file, and a folder is'
    + ' excluded with an exclusion rule instead',
  ));
  await mount([]);
  await type('SUB/*.txt');
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe('The mask SUB/*.txt was not added. This is what the check answered:');
  expect(screen.queryByText(CASE_NOTE)).toBeNull();
});

// 🔴 Removal is a disclosure, not a tidy-up: it is the one press on this screen
// that sends more text to a third party.
test('removing states the inverse cost before it removes anything', async () => {
  await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.pdf' }));

  expect(visibleText(screen.getByTestId('mask-confirm-cost'))).toBe(
    'From the next scan of each folder on, the files this mask was holding back are indexed'
    + ' again, and their text is sent to the model provider.',
  );
  expect(removeMask).not.toHaveBeenCalled();
  // No count is asked for a removal: `mask_preview` answers what a mask REMOVES
  // and would be the wrong number entirely for what removing it releases.
  expect(maskPreview).not.toHaveBeenCalled();
});

test('removing re-reads the list rather than editing the array on screen', async () => {
  // Two masks, so "the list re-reads" can be told from "the list happened to be
  // right": a component that spliced its own array would also lose `*.pdf`.
  removeMask.mockResolvedValue(true);
  await mount(['*.pdf', '*.tmp']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  // The shell answers with a list that differs from a spliced one in BOTH
  // directions: `*.pdf` is gone and `*.log` is there, and no local edit could
  // have invented `*.log`.
  listMasks.mockResolvedValue(['*.tmp', '*.log']);
  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.pdf' }));
  await fireEvent.click(confirmRemove('*.pdf'));

  await waitFor(() => expect(screen.getByText('*.log')).toBeTruthy());
  expect(removeMask).toHaveBeenCalledWith('*.pdf');
  expect(listMasks).toHaveBeenCalledTimes(2);
  expect(screen.queryByText('*.pdf')).toBeNull();
  expect(screen.getByText('*.tmp')).toBeTruthy();
});

test('a mask another window removed first says so, and one this window removed does not', async () => {
  removeMask.mockResolvedValue(false); // the row was already gone
  await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  listMasks.mockResolvedValue([]);
  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.pdf' }));
  await fireEvent.click(confirmRemove('*.pdf'));

  await waitFor(() => expect(screen.getByTestId('mask-already-gone')).toBeTruthy());
  expect(screen.getByTestId('mask-already-gone').textContent)
    .toBe('There was no such mask left to remove. The list has been re-read.');

  // The other direction, on a second removal that does remove something: the
  // note must not stand on a press it does not describe.
  cleanup();
  removeMask.mockResolvedValue(true);
  await mount(['*.tmp']);
  await waitFor(() => expect(screen.getByText('*.tmp')).toBeTruthy());
  listMasks.mockResolvedValue([]);
  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.tmp' }));
  await fireEvent.click(confirmRemove('*.tmp'));

  await waitFor(() => expect(screen.queryByText('*.tmp')).toBeNull());
  expect(screen.queryByTestId('mask-already-gone')).toBeNull();
});

// Task 11 fix round 1, F5. `<label for="mask-draft-input">` names the field's
// accessible name; `screen.getByRole('textbox')` (what `type()` above uses)
// finds the one input on the page regardless of whether the `for` actually
// points at it, so no existing test would notice the id going stale. This
// finds it by the LABEL, which only works if the association is real.
test('the mask input is really named by its own label, not just found by being the only textbox', async () => {
  await mount([]);
  expect(screen.getByLabelText('New mask:')).toBe(screen.getByRole('textbox'));
});

// The empty string is `validate_mask`'s one deliberate non-error: it previews
// as two zeros and `add_mask` refuses it. The blank row gets no press at all,
// so neither answer is ever reached — and whitespace is NOT blank, because
// `add_mask` refuses `"   "` with a sentence of its own and trimming here would
// hand the person the wrong one of the two.
test('the blank row cannot be pressed, and whitespace is not blank', async () => {
  await mount([]);

  expect((addButton() as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(addButton());
  expect(maskPreview).not.toHaveBeenCalled();

  maskPreview.mockRejectedValue(new Error('file mask "   " begins or ends with whitespace — remove it'));
  await type('   ');
  expect((addButton() as HTMLButtonElement).disabled).toBe(false);
  await fireEvent.click(addButton());

  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(screen.getByTestId('mask-refused-reason').textContent)
    .toBe('file mask "   " begins or ends with whitespace — remove it');
  expect(maskPreview).toHaveBeenCalledWith('   ');
});

// The list read is reachable from three places — mount, an add and a remove —
// so two `list_masks` calls can be on the wire at once, and the one asked for
// first is not the one that has to answer first.
test('the older of two overlapping list reads writes nothing', async () => {
  let releaseMount: (v: string[]) => void = () => {};
  const mountRead = new Promise<string[]>((resolve) => { releaseMount = resolve; });
  setLocale('en'); // seed, do not inherit
  listMasks.mockReturnValueOnce(mountRead); // still in flight for the whole test
  maskPreview.mockResolvedValue({ paths: 1, documents: 1 });
  addMask.mockResolvedValue({ kind: 'stored' });

  render(Masks);
  await waitFor(() => expect(listMasks).toHaveBeenCalledTimes(1));

  // An add starts a newer read while the mount read is still on the wire, and
  // the newer one answers first.
  listMasks.mockResolvedValue(['*.tmp']);
  await type('*.tmp');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.tmp'));
  await waitFor(() => expect(screen.getByText('*.tmp')).toBeTruthy());

  // The mount read answers last, with the list as it was before the add.
  releaseMount(['*.stale']);
  await mountRead;
  await Promise.resolve();
  await Promise.resolve();

  // Both directions: the superseded answer is not drawn, and the newer one is
  // still standing rather than having been replaced by an empty list.
  expect(screen.queryByText('*.stale')).toBeNull();
  expect(screen.getByText('*.tmp')).toBeTruthy();
});

test('a list that cannot be read says so, and does not claim there are no masks', async () => {
  setLocale('en'); // seed, do not inherit
  listMasks.mockRejectedValue(new Error('the index is not open'));

  const { container } = render(Masks);

  await waitFor(() => expect(screen.getByTestId('masks-load-reason')).toBeTruthy());
  expect(screen.getByTestId('masks-load-reason').textContent).toBe('the index is not open');
  expect(screen.getByText('The list of masks could not be read.')).toBeTruthy();
  // 🔴 The two states must not be confused: "no mask is stored" and "the list
  // could not be read" are opposite claims about the person's protection.
  expect(visibleText(container)).not.toContain('No file mask has been added yet.');
});

// Task 11 fix round 1, F5. `refresh` writes `masks = list; loadError = null;`
// together (`Masks.svelte:69-70`) — if the second half regressed, a failed
// first read leaves `loadError` set forever, because `{#if loadError}` wins
// over the list branch and a LATER successful read has no way back onto the
// screen. Provoked here through the add flow's own `await refresh()`, the
// only other caller besides `onMount`.
test('a read that fails is not the last word: a later successful read replaces it', async () => {
  setLocale('en'); // seed, do not inherit
  listMasks.mockRejectedValueOnce(new Error('the index is not open'));
  maskPreview.mockResolvedValue({ paths: 1, documents: 1 });
  addMask.mockResolvedValue({ kind: 'stored' });

  render(Masks);
  await waitFor(() => expect(screen.getByTestId('masks-load-reason')).toBeTruthy());

  listMasks.mockResolvedValue(['*.pdf']);
  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.pdf'));

  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());
  // Both directions: the error message is gone AND the list it was blocking is
  // now the thing on screen.
  expect(screen.queryByTestId('masks-load-reason')).toBeNull();
  expect(screen.queryByText('The list of masks could not be read.')).toBeNull();
});

// 🔴 The whole section, read as a person reads it. Everything below is one
// screen: the heading, the sentence that a mask is global and lands on each
// folder's own next scan, the case ruling, the stored masks, and the controls.
test('the whole section reads as one screen', async () => {
  const { container } = await mount(['*.pdf', '*.tmp']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  expect(visibleText(container)).toBe(
    'File masks'
    + ' A mask applies to every watched folder at once: it is compared with a file name, at any'
    + ' depth. Each folder applies it on its own next scan. Letter case does not matter, so *.PDF'
    + ' and *.pdf are one and the same rule; neither does the way a name happens to store its'
    + ' accents.'
    + ' *.pdf Remove'
    + ' *.tmp Remove'
    + ' New mask: Add a mask',
  );
});

// 🔴 "No mask is stored" and "nobody has answered yet" are different claims,
// and the first one printed on a screen that does not know it yet is a claim
// about the person's protection.
test('the section does not claim the list is empty while the first read is still on the wire', async () => {
  let release: (v: string[]) => void = () => {};
  setLocale('en'); // seed, do not inherit
  listMasks.mockReturnValue(new Promise<string[]>((resolve) => { release = resolve; }));

  const { container } = render(Masks);
  await waitFor(() => expect(listMasks).toHaveBeenCalled());
  expect(visibleText(container)).not.toContain('No file mask has been added yet.');

  release([]);
  await waitFor(() => expect(screen.getByText('No file mask has been added yet.')).toBeTruthy());
});

// The catalogue is not enough on its own: a `$derived` without `void $locale`
// caches its English value and keeps it through a switch.
test('every sentence on the section follows a language switch, the standing question included', async () => {
  maskPreview.mockResolvedValue({ paths: 4, documents: 0 });
  const { container } = await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  // Read once under 'en' BEFORE the switch, so a cached English value is a
  // value that was genuinely read: the mutant only dies if the read after the
  // switch is a later one.
  await type('*.tmp');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  expect(visibleText(container)).toContain('As of now, at least 4 files');

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('Маски файлів')).toBeTruthy());

  const text = visibleText(container);
  expect(text).toContain(t('settings_masks_explainer'));
  expect(text).toContain('Станом на зараз під цю маску підпадає щонайменше 4 файли');
  expect(text).toContain(t('settings_masks_confirm_add_heading', { mask: '*.tmp' }));
  // The accessible names follow the switch too, and they are what tells two
  // otherwise identical controls apart.
  expect(screen.getByRole('button', { name: 'Підтвердити додавання маски *.tmp' }).textContent)
    .toBe('Підтвердити');
  expect(screen.getByRole('button', { name: 'Видалити маску *.pdf' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Додати маску' })).toBeTruthy();
  expect(text).not.toContain('As of now');
});

// Task 11 fix round 1, F5. Two more `$derived`s the switch test above never
// reached: `alreadyGoneLabel` (`:176-179`) and `refusal` (`:196-210`). Each is
// checked in its OWN mount rather than one shared screen: `forget()` clears
// `refused`/`actionError`/`alreadyGone` TOGETHER at the start of every new
// `askAdd`/`askRemove`, and setting either one always requires a `pending`
// that is `null` by then — so a refusal and an already-gone note, or either of
// them alongside a standing question, can never be on screen at once. (The
// review that asked for this test suggested reading both off a single screen;
// probing the running component first showed that state is unreachable, so
// this covers the same two derived values across two mounts instead.)
test('the refusal frame and the already-gone note also follow a language switch', async () => {
  maskPreview.mockRejectedValue(new Error('the index is not open'));
  const { container } = await mount([]);

  await type('*.pdf');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-refused-reason')).toBeTruthy());
  expect(visibleText(container)).toContain(t('settings_masks_refused_add', { mask: '*.pdf' }));

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('Маски файлів')).toBeTruthy());
  expect(screen.getByTestId('mask-refused-heading').textContent)
    .toBe(t('settings_masks_refused_add', { mask: '*.pdf' }));
  expect(visibleText(container)).not.toContain('was not added');

  cleanup();
  setLocale('en');
  removeMask.mockResolvedValue(false); // the row was already gone
  const { container: container2 } = await mount(['*.tmp']);
  await waitFor(() => expect(screen.getByText('*.tmp')).toBeTruthy());

  await fireEvent.click(screen.getByRole('button', { name: 'Remove the mask *.tmp' }));
  await fireEvent.click(confirmRemove('*.tmp'));
  await waitFor(() => expect(screen.getByTestId('mask-already-gone')).toBeTruthy());
  expect(visibleText(container2)).toContain(t('settings_masks_already_gone'));

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('Маски файлів')).toBeTruthy());
  expect(screen.getByTestId('mask-already-gone').textContent).toBe(t('settings_masks_already_gone'));
  expect(visibleText(container2)).not.toContain('There was no such mask');

  // Task 11 fix round 2, F3. The third `$derived` of the same shape, in its own
  // mount for the reason above: `forget()` clears this one together with the
  // other two, so it can never share a screen with either.
  cleanup();
  setLocale('en');
  maskPreview.mockResolvedValue({ paths: 1, documents: 1 });
  addMask.mockResolvedValue({ kind: 'alreadyStored', stored: '*.pdf' });
  const { container: container3 } = await mount(['*.pdf']);
  await waitFor(() => expect(screen.getByText('*.pdf')).toBeTruthy());

  await type('*.PDF');
  await fireEvent.click(addButton());
  await waitFor(() => expect(screen.getByTestId('mask-confirm-cost')).toBeTruthy());
  await fireEvent.click(confirmAdd('*.PDF'));
  await waitFor(() => expect(screen.getByTestId('mask-already-stored')).toBeTruthy());
  expect(visibleText(container3)).toContain(t('settings_masks_already_stored', { stored: '*.pdf' }));

  setLocale('uk');
  await waitFor(() => expect(screen.getByText('Маски файлів')).toBeTruthy());
  expect(screen.getByTestId('mask-already-stored').textContent)
    .toBe(t('settings_masks_already_stored', { stored: '*.pdf' }));
  expect(visibleText(container3)).not.toContain('You already have this rule');
});
