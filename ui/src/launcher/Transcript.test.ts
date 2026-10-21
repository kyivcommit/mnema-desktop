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
