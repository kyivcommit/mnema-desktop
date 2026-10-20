// The bench's handlers are typed `Record<string, (a) => unknown>` and nothing
// else imports this file, so neither `svelte-check` nor the component tests
// (which mock `ipc` themselves) notice a command whose answer has drifted from
// what the window reads. This drives the same `invoke` the launcher does,
// through the bench's own IPC mock, and pins the two shapes that broke: `ask`
// now answers `AskReply`, and `open_document` must point the pane somewhere.
import { invoke } from '@tauri-apps/api/core';
import { expect, test } from 'vitest';
import { installMockBackend } from './mock-backend';

installMockBackend('launcher');

test('ask answers with the reply the launcher reads', async () => {
  const reply = await invoke<{ answer: { kind: string }; historyId: unknown }>('ask', { query: 'demo' });
  // `Launcher.svelte` reads `reply.answer` and marks `reply.historyId`; the bare
  // `AskAnswer` the bench used to return made `stateFromAnswer(undefined)` throw.
  expect(reply.answer.kind).toBe('generated');
  expect(typeof reply.historyId).toBe('number');
});

test('open_document points the pane at a block it can show', async () => {
  const view = await invoke<{
    focusBlockIds: number[];
    spans: Array<{ blockId: number; primary: boolean }>;
  }>('open_document', {});
  // Empty arrays here were every citation click answering "the passage is gone".
  expect(view.focusBlockIds.length).toBeGreaterThan(0);
  expect(view.spans[0]?.blockId).toBe(view.focusBlockIds[0]);
  expect(view.spans[0]?.primary).toBe(true);
});
