import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, test } from 'vitest';
import Transcript from './Transcript.svelte';
import { generated } from '../lib/fixtures';
import type { AskAnswer, AskCitation, Hit } from '../lib/ipc';

const HERE = dirname(fileURLToPath(import.meta.url));

// The stylesheet is read as a real sheet, so `align-self` is the browser's
// computed answer and not a class name a test greps for.
let sheets: HTMLStyleElement[] = [];
beforeEach(() => {
  for (const path of ['base.css', 'launcher.css']) {
    const style = document.createElement('style');
    style.textContent = readFileSync(join(HERE, '../styles', path), 'utf8');
    document.head.append(style);
    sheets.push(style);
  }
});
afterEach(() => { cleanup(); sheets.forEach((s) => s.remove()); sheets = []; });

test('a_turn_shows_the_question_right_and_the_answer_left_without_labels', () => {
  const answer = generated as Extract<AskAnswer, { kind: 'generated' }>;
  const { container } = render(Transcript, {
    turns: [{ query: 'how much?', answer }],
    state: { kind: 'idle' },
  });

  // The question is the only thing pushed to the right edge…
  const question = screen.getByTestId('query-echo');
  expect(question.textContent).toBe('how much?');
  expect(getComputedStyle(question).alignSelf).toBe('flex-end');

  // …the answer keeps the column's own alignment and has no bubble of its own.
  const body = screen.getByTestId('answer-body');
  expect(body.closest('.umsg')).toBeNull();
  expect(getComputedStyle(body.closest('.answer-card')!).alignSelf).not.toBe('flex-end');

  // Role is told by alignment alone: no speaker labels anywhere in the turn.
  expect(container.textContent).not.toMatch(/Асистент|Assistant/);
  expect(container.textContent).not.toContain('Ви');
});

// Task 7: this is the wire the launcher's panel listens on. A press on a
// preview reports THAT citation, which is what moves the panel; mounting the
// turn reports nothing (a stored answer opened from Recents must not move a
// pane the person was reading).
test('a_turn_reports_the_card_that_was_pressed', async () => {
  const answer = generated as Extract<AskAnswer, { kind: 'generated' }>;
  const seen: Array<AskCitation | Hit | null> = [];
  render(Transcript, {
    turns: [{ query: 'how much?', answer }],
    state: { kind: 'idle' },
    onSelected: (selection) => seen.push(selection),
  });
  const reported = () => (seen.at(-1) as { chunkId: number } | null)?.chunkId;

  expect(seen).toHaveLength(0);
  await fireEvent.click(screen.getByTestId('preview-7'));
  await waitFor(() => expect(reported()).toBe(43)); // citationB, the pressed one
});

// Owner, 2026-10-22: after Enter the question waits in the transcript with dots
// for the answer, scrolled into view. Both directions: no pending turn once the
// ask is over.
test('a_question_in_flight_is_drawn_with_dots_and_scrolled_to', async () => {
  const answer = generated as Extract<AskAnswer, { kind: 'generated' }>;
  const writes: number[] = [];
  const proto = HTMLElement.prototype;
  Object.defineProperty(proto, 'scrollTop', { configurable: true, get: () => 0, set: (v: number) => { writes.push(v); } });
  Object.defineProperty(proto, 'scrollHeight', { configurable: true, get: () => 900 });
  try {
    const { rerender } = render(Transcript, {
      turns: [{ query: 'how much?', answer }],
      state: { kind: 'inFlight', query: 'and then?' },
    });
    const pending = screen.getByTestId('pending-turn');
    expect(getComputedStyle(pending.querySelector('.umsg')!).alignSelf).toBe('flex-end');
    expect(pending.querySelector('.umsg')!.textContent).toBe('and then?');
    expect(pending.querySelectorAll('.typing span')).toHaveLength(3);
    expect(writes).toContain(900);

    await rerender({ turns: [{ query: 'how much?', answer }], state: { kind: 'idle' } });
    expect(screen.queryByTestId('pending-turn')).toBeNull();
  } finally {
    delete (proto as { scrollTop?: number }).scrollTop;
    delete (proto as { scrollHeight?: number }).scrollHeight;
  }
});

// Owner, 2026-10-22: the answer that arrives puts its question at the top of
// the view. Both directions: an ask that drew nothing new does not move it.
test('an_arrived_answer_scrolls_its_question_to_the_top', async () => {
  const answer = generated as Extract<AskAnswer, { kind: 'generated' }>;
  const second = structuredClone(answer);
  const writes: number[] = [];
  const proto = HTMLElement.prototype;
  const rect = Element.prototype.getBoundingClientRect;
  Object.defineProperty(proto, 'scrollTop', { configurable: true, get: () => 100, set: (v: number) => { writes.push(v); } });
  Object.defineProperty(proto, 'scrollHeight', { configurable: true, get: () => 900 });
  // The scroller sits at 50; the newest exchange starts 400 below the page top.
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const top = this.getAttribute('data-testid') === 'transcript' ? 50 : 400;
    return { top } as DOMRect;
  };
  try {
    const first = [{ query: 'how much?', answer }];
    const { rerender } = render(Transcript, { turns: first, state: { kind: 'inFlight', query: 'and then?' } });
    await rerender({ turns: [...first, { query: 'and then?', answer: second }], state: { kind: 'generated', query: 'and then?', answer: second } });
    expect(writes.at(-1)).toBe(100 + 400 - 50);

    writes.length = 0;
    await rerender({ turns: [...first, { query: 'and then?', answer: second }], state: { kind: 'inFlight', query: 'refused?' } });
    await rerender({ turns: [...first, { query: 'and then?', answer: second }], state: { kind: 'idle' } });
    expect(writes).toEqual([900]); // the jump down on send, and nothing after
  } finally {
    delete (proto as { scrollTop?: number }).scrollTop;
    delete (proto as { scrollHeight?: number }).scrollHeight;
    Element.prototype.getBoundingClientRect = rect;
  }
});
