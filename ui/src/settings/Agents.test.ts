import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Agents from './Agents.svelte';
import { setLocale } from '../i18n';
import type { AgentLogEntry, Change, ClientId, ClientState } from '../lib/ipc';

const agentAccessEnabled = vi.fn();
const setAgentAccess = vi.fn();
const agentLog = vi.fn();
const agentClients = vi.fn();
const applyAgentClients = vi.fn();

vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('../lib/ipc', () => ({
  agentAccessEnabled: () => agentAccessEnabled(),
  setAgentAccess: (on: boolean) => setAgentAccess(on),
  agentLog: () => agentLog(),
  agentClients: () => agentClients(),
  applyAgentClients: (changes: Change[]) => applyAgentClients(changes),
}));

const entry = (over: Partial<AgentLogEntry>): AgentLogEntry => ({ at: '2026-01-02T03:04:05Z', op: 'search', ...over });

// One client as `agent_clients` sends it: `link` is the CLI's own record about
// the `mnema` server, null when the record could not be read at all, and
// `error` carries that read's refusal.
const client = (id: ClientId, over: Partial<ClientState> = {}): ClientState =>
  ({ id, installed: true, link: { kind: 'absent' }, error: null, ...over });

// Nothing to change and nothing outdated: every installed client is either
// connected with this build's own path or not connected at all.
const SETTLED: ClientState[] = [
  client('claude', { link: { kind: 'current' } }),
  client('codex'),
  client('pi'),
  client('opencode'),
];

// The state the section opens on in the mock bench: one connected, one absent,
// one connected with another build's path, one CLI that is not on this machine.
const MIXED: ClientState[] = [
  client('claude', { link: { kind: 'current' } }),
  client('codex'),
  client('pi', { link: { kind: 'stale', command: '/old/Mnema.app/Contents/MacOS/mnema' } }),
  client('opencode', { installed: false, link: null }),
];

const box = (id: ClientId) => screen.getByTestId(`agents-client-${id}`) as HTMLInputElement;
const apply = () => screen.getByTestId('agents-apply') as HTMLButtonElement;

beforeEach(() => {
  setLocale('en');
  agentAccessEnabled.mockReset().mockResolvedValue(false);
  setAgentAccess.mockReset().mockResolvedValue(true);
  agentLog.mockReset().mockResolvedValue([]);
  agentClients.mockReset().mockResolvedValue(SETTLED);
  applyAgentClients.mockReset().mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

test('access is off by default and no agent is offered', async () => {
  render(Agents);
  const access = screen.getByTestId('agents-access') as HTMLInputElement;
  await waitFor(() => expect(agentAccessEnabled).toHaveBeenCalled());
  expect(access.checked).toBe(false);
  expect(screen.queryByTestId('agents-client-claude')).toBeNull();
  expect(screen.queryByTestId('agents-apply')).toBeNull();
  expect(agentClients).not.toHaveBeenCalled();
});

test('turning access on reads the four agents and draws one box each', async () => {
  render(Agents);
  agentAccessEnabled.mockResolvedValue(true);
  await fireEvent.click(screen.getByTestId('agents-access'));

  await waitFor(() => expect(agentClients).toHaveBeenCalledTimes(1));
  expect(setAgentAccess).toHaveBeenCalledWith(true);
  for (const name of ['Claude Code', 'Codex', 'pi', 'opencode']) {
    expect(screen.getByText(name)).toBeTruthy();
  }
  await waitFor(() => expect(box('claude').checked).toBe(true));
  // Connected and not connected, from the CLI's own record and not from a request.
  expect(box('claude').checked).toBe(true);
  expect(box('codex').checked).toBe(false);
});

test('a client whose CLI is not installed is disabled and says so', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue(MIXED);
  render(Agents);

  await waitFor(() => expect(box('opencode')).toBeTruthy());
  // The note is the signal that the answer has landed: before it, every box is
  // disabled for the loading reason and an assertion about this one proves nothing.
  const note = await screen.findByTestId('agents-note-opencode');
  expect(note.textContent).toBe('not installed');
  expect(box('opencode').disabled).toBe(true);
  expect(box('claude').disabled).toBe(false);
});

test('a client whose state could not be read shows the error, not "not installed"', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue([
    client('claude', { link: { kind: 'current' } }),
    client('codex', { installed: false, link: null, error: 'the login shell was not asked for its PATH' }),
    client('pi'),
    client('opencode'),
  ]);
  render(Agents);

  await waitFor(() => expect(box('codex').disabled).toBe(true));
  // The error wins over the absence: the shell could not say what is installed,
  // which is a different sentence from "this CLI is not here".
  expect(screen.queryByTestId('agents-note-codex')).toBeNull();
  expect(screen.queryByText('not installed')).toBeNull();
  const details = await screen.findByTestId('agents-error-codex');
  expect(details.textContent).toContain('Details');
  expect(details.textContent).toContain('the login shell was not asked for its PATH');
});

test('a connected client with another path is ticked and says the path is outdated', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue(MIXED);
  render(Agents);

  await waitFor(() => expect(box('pi').checked).toBe(true));
  expect(box('pi').disabled).toBe(false);
  expect(screen.getByTestId('agents-note-pi').textContent).toBe('outdated path');
});

test('Apply is disabled until the person changes a box', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  render(Agents);

  await waitFor(() => expect(box('codex').checked).toBe(false));
  expect(apply().disabled).toBe(true);
  await fireEvent.click(box('codex'));
  expect(apply().disabled).toBe(false);
  // Back to the state the CLI reported: nothing left to apply.
  await fireEvent.click(box('codex'));
  expect(apply().disabled).toBe(true);
});

test('an outdated path alone enables Apply', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue(MIXED);
  render(Agents);

  await waitFor(() => expect(box('pi').checked).toBe(true));
  // The choice equals the state, and Apply is still owed: the record names
  // another build's binary, which only an add can correct.
  expect(apply().disabled).toBe(false);
});

test('Apply sends a change per box the person moved, and nothing for the rest', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue([
    client('claude', { link: { kind: 'current' } }), // untouched
    client('codex', { link: { kind: 'current' } }), // the person unticks it
    client('pi', { link: { kind: 'stale', command: '/old/Mnema.app/Contents/MacOS/mnema' } }),
    client('opencode'), // the person ticks it
  ]);
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  await fireEvent.click(box('opencode'));
  await fireEvent.click(apply());

  await waitFor(() => expect(applyAgentClients).toHaveBeenCalledTimes(1));
  // In the order the boxes are drawn. Claude Code is absent because nothing
  // about it moved; pi is there with nothing moved only because a stale record
  // is a change an add corrects.
  expect(applyAgentClients).toHaveBeenCalledWith([
    { id: 'codex', connect: false },
    { id: 'pi', connect: true },
    { id: 'opencode', connect: true },
  ]);
});

test('a client whose state could not be read is never sent, even when Apply is about another', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  // Claude's `mcp get` timed out once: installed, but no record was read, so the
  // box is disabled and unticked. Unticked here must not reach the backend as
  // "disconnect Claude", which is what a "wanted set" would have meant.
  agentClients.mockResolvedValue([
    client('claude', { link: null, error: 'timed out after 15 s' }),
    client('codex'),
    client('pi'),
    client('opencode'),
  ]);
  render(Agents);

  // The error fold is what says the answer landed; a wait on an unchecked box
  // is also true while the first read is still out.
  await screen.findByTestId('agents-error-claude');
  expect(box('claude').disabled).toBe(true);
  await fireEvent.click(box('codex'));
  await fireEvent.click(apply());

  await waitFor(() => expect(applyAgentClients).toHaveBeenCalledTimes(1));
  expect(applyAgentClients).toHaveBeenCalledWith([{ id: 'codex', connect: true }]);
});

test('a press the command did not answer is its own sentence, not a failed read', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  applyAgentClients.mockRejectedValue('the command is not registered');
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  await fireEvent.click(apply());

  const rejected = await screen.findByTestId('agents-apply-rejected');
  expect(rejected.textContent).toContain('Could not apply the changes');
  expect(rejected.textContent).toContain('the command is not registered');
  // The state itself was re-read and answered: nothing here may say it was not.
  expect(screen.queryByTestId('agents-read-failed')).toBeNull();
});

test('a rejected press is reported at once, without waiting for the re-read', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  applyAgentClients.mockRejectedValue('the command is not registered');
  // The re-read never answers. The press's own sentence must not be held behind
  // four CLI reads (up to about twenty seconds) that may never settle.
  agentClients.mockResolvedValueOnce(SETTLED).mockReturnValueOnce(new Promise(() => {}));
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  await fireEvent.click(apply());

  expect((await screen.findByTestId('agents-apply-rejected')).textContent)
    .toContain('the command is not registered');
});

test('a re-read that disables a box drops the pick that box was carrying', async () => {
  // The switch goes off and on again, and the second read finds Codex broken:
  // the tick was made against a record that no longer holds, and a disabled box
  // must not stay ticked.
  agentAccessEnabled
    .mockResolvedValueOnce(true) // mount
    .mockResolvedValueOnce(false) // the switch is turned off
    .mockResolvedValue(true); // and on again
  agentClients
    .mockResolvedValueOnce(SETTLED)
    .mockResolvedValueOnce([
      client('claude', { link: { kind: 'current' } }),
      client('codex', { link: null, error: 'codex exited with code 2' }),
      client('pi'),
      client('opencode'),
    ]);
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  expect(box('codex').checked).toBe(true);
  const access = screen.getByTestId('agents-access') as HTMLInputElement;
  await fireEvent.click(access); // off
  await waitFor(() => expect(access.checked).toBe(false));
  await fireEvent.click(access); // on: a fresh read, and Codex is broken now

  await waitFor(() => expect(box('codex').disabled).toBe(true));
  expect(screen.getByTestId('agents-error-codex')).toBeTruthy();
  expect(box('codex').checked).toBe(false);
});

test('a failed re-read keeps the pick and says the state is unknown', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValueOnce(SETTLED).mockRejectedValueOnce('read failed');
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  await fireEvent.click(apply());

  expect((await screen.findByTestId('agents-read-failed')).textContent).toContain('read failed');
  // Nothing confirmed the press, so the tick stays where the person put it.
  expect(box('codex').checked).toBe(true);
  expect(box('codex').disabled).toBe(false);
});

test('after Apply the boxes show the re-read state, not what was asked for', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  // The second read disagrees with the request on purpose: Claude Code lost its
  // record while Codex did not gain one. A window that drew its own request
  // would show the opposite of both.
  agentClients
    .mockResolvedValueOnce(SETTLED)
    .mockResolvedValueOnce([
      client('claude'),
      client('codex'),
      client('pi'),
      client('opencode'),
    ]);
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  await fireEvent.click(box('codex'));
  await fireEvent.click(box('claude'));
  await fireEvent.click(apply());

  // Codex is the box that tells the two apart: the person ticked it, and the
  // record came back without it.
  await waitFor(() => expect(box('codex').checked).toBe(false));
  expect(box('claude').checked).toBe(false);
  await waitFor(() => expect(agentClients).toHaveBeenCalledTimes(2));
  // The person's own pick is gone with the answer it was made against: the
  // boxes follow the CLIs again.
  expect(apply().disabled).toBe(true);
});

test('one client failing is named with its direction and does not hide another', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue([
    client('claude', { link: { kind: 'current' } }),
    client('codex'),
    client('pi', { link: { kind: 'current' } }),
    client('opencode'),
  ]);
  // One answer per client the press named: Codex was ticked, pi unticked, and
  // Claude Code was not touched at all, so it is not in the reply.
  applyAgentClients.mockResolvedValue([
    { id: 'codex', error: 'codex: No MCP server named "mnema".' },
    { id: 'pi', error: 'pi: could not write the global config' },
  ]);
  render(Agents);

  await waitFor(() => expect(box('codex').checked).toBe(false));
  await fireEvent.click(box('codex'));
  await fireEvent.click(box('pi'));
  await fireEvent.click(apply());

  const codex = await screen.findByTestId('agents-failed-codex');
  const pi = screen.getByTestId('agents-failed-pi');
  expect(codex.getAttribute('role')).toBe('alert');
  expect(codex.textContent).toContain('Could not connect Codex');
  expect(screen.getByTestId('agents-failed-details-codex').textContent)
    .toContain('codex: No MCP server named "mnema".');
  // The other direction is a different sentence: this client was being taken
  // out, not put in.
  expect(pi.textContent).toContain('Could not disconnect pi');
  expect(screen.getByTestId('agents-failed-details-pi').textContent)
    .toContain('pi: could not write the global config');
  // Claude Code answered with no error: nothing is said about it.
  expect(screen.queryByTestId('agents-failed-claude')).toBeNull();
});

test('no command line, JSON fragment or copy button is left in the section', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockResolvedValue(MIXED);
  render(Agents);

  await waitFor(() => expect(box('claude').checked).toBe(true));
  expect(document.querySelector('[data-testid^="agents-copy"]')).toBeNull();
  expect(screen.queryByTestId('agents-cmd')).toBeNull();
  expect(document.body.textContent).not.toContain('mcp add');
  expect(document.body.textContent).not.toContain('mcpServers');
});

test('the four boxes are drawn disabled while the first answer is on its way', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockReturnValue(new Promise(() => {}));
  render(Agents);

  await waitFor(() => expect(box('claude')).toBeTruthy());
  for (const id of ['claude', 'codex', 'pi', 'opencode'] as const) {
    expect(box(id).disabled).toBe(true);
  }
  expect(apply().disabled).toBe(true);
});

test('a failed read of the clients says so and leaves the boxes disabled', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentClients.mockRejectedValue('the command is not registered');
  render(Agents);

  const sentence = await screen.findByTestId('agents-read-failed');
  expect(sentence.textContent).toContain('Could not read the agent state');
  expect(sentence.textContent).toContain('the command is not registered');
  expect(box('claude').disabled).toBe(true);
  expect(apply().disabled).toBe(true);
});

test('the boxes and Apply are disabled while Apply is running', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  let release!: (rows: { id: ClientId; error: string | null }[]) => void;
  applyAgentClients.mockReturnValue(new Promise((r) => (release = r)));
  render(Agents);

  await waitFor(() => expect(box('codex').checked).toBe(false));
  await fireEvent.click(box('codex'));
  await fireEvent.click(apply());

  expect(box('codex').disabled).toBe(true);
  expect(apply().disabled).toBe(true);
  release([]);
  await waitFor(() => expect(box('codex').disabled).toBe(false));
});

test('a refused start shows the error and does not claim access is on', async () => {
  setAgentAccess.mockRejectedValue('Cannot bind the agent socket');
  render(Agents);
  await fireEvent.click(screen.getByTestId('agents-access'));
  await waitFor(() => expect(screen.getByTestId('agents-failure').textContent).toContain('Cannot bind the agent socket'));
  expect((screen.getByTestId('agents-access') as HTMLInputElement).checked).toBe(false);
  expect(screen.queryByTestId('agents-client-claude')).toBeNull();
});

test('the log lists entries newest first with the claimed client name', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentLog.mockResolvedValue([
    entry({ op: 'search', query: 'newest question', passages: 3, client: 'synthetic-agent' }),
    entry({ op: 'open', query: 'older question' }),
  ]);
  render(Agents);
  const list = await screen.findByTestId('agents-log');
  const items = list.querySelectorAll('li');
  expect(items).toHaveLength(2);
  expect(items[0].textContent).toContain('newest question');
  expect(items[0].textContent).toContain('3 passages');
  expect(items[0].textContent).toContain('identified as: synthetic-agent');
  expect(items[1].textContent).toContain('older question');
  expect(items[1].textContent).not.toContain('identified as');
});

test('the log names an op in the language of the window, and dates it the same way', async () => {
  agentAccessEnabled.mockResolvedValue(true);
  agentLog.mockResolvedValue([entry({ op: 'passage' })]);
  setLocale('uk');
  render(Agents);
  const item = (await screen.findByTestId('agents-log')).querySelector('li')!;
  expect(item.textContent).toContain('уривок');
  expect(item.textContent).not.toContain('passage');
  expect(item.querySelector('time')!.textContent).toBe(new Date('2026-01-02T03:04:05Z').toLocaleString('uk'));
  cleanup();
  setLocale('en');
  render(Agents);
  const en = (await screen.findByTestId('agents-log')).querySelector('li')!;
  expect(en.textContent).toContain('passage');
  expect(en.querySelector('time')!.textContent).toBe(new Date('2026-01-02T03:04:05Z').toLocaleString('en'));
});

test('an empty log says no agent has asked yet', async () => {
  render(Agents);
  expect(await screen.findByText('No agent has asked yet')).toBeTruthy();
});

test('the log is polled every two seconds while mounted, and no longer after', async () => {
  vi.useFakeTimers();
  agentAccessEnabled.mockResolvedValue(true);
  const { unmount } = render(Agents);
  await vi.advanceTimersByTimeAsync(0);
  const base = agentLog.mock.calls.length;
  await vi.advanceTimersByTimeAsync(2000);
  expect(agentLog.mock.calls.length).toBe(base + 1);
  await vi.advanceTimersByTimeAsync(2000);
  expect(agentLog.mock.calls.length).toBe(base + 2);
  unmount();
  await vi.advanceTimersByTimeAsync(10000);
  expect(agentLog.mock.calls.length).toBe(base + 2);
});

test('the log is not polled while access is off', async () => {
  vi.useFakeTimers();
  render(Agents);
  await vi.advanceTimersByTimeAsync(10000);
  expect(agentLog).not.toHaveBeenCalled();
});

test('a failed read of the switch keeps the last known state and shows the error', async () => {
  agentAccessEnabled.mockResolvedValueOnce(true).mockRejectedValue('read failed');
  render(Agents);
  const access = screen.getByTestId('agents-access') as HTMLInputElement;
  await waitFor(() => expect(access.checked).toBe(true));
  await fireEvent.click(access); // asks for off; the re-read then fails
  await waitFor(() => expect(screen.getByTestId('agents-failure').textContent).toContain('read failed'));
  expect(access.checked).toBe(true);
});

test('a start that answers false without an error leaves the switch unchecked', async () => {
  setAgentAccess.mockResolvedValue(false);
  render(Agents);
  const access = screen.getByTestId('agents-access') as HTMLInputElement;
  await fireEvent.click(access);
  await waitFor(() => expect(agentAccessEnabled.mock.calls.length).toBeGreaterThan(1));
  expect(access.checked).toBe(false);
  expect(screen.queryByTestId('agents-failure')).toBeNull();
});

test('the switch is disabled while a change is pending', async () => {
  let release!: (v: boolean) => void;
  setAgentAccess.mockReturnValue(new Promise<boolean>((r) => (release = r)));
  render(Agents);
  const access = screen.getByTestId('agents-access') as HTMLInputElement;
  await fireEvent.click(access);
  expect(access.disabled).toBe(true);
  release(false);
  await waitFor(() => expect(access.disabled).toBe(false));
});
