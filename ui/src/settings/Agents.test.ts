import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Agents from './Agents.svelte';
import { setLocale } from '../i18n';
import type { AgentLogEntry } from '../lib/ipc';

const agentAccessEnabled = vi.fn();
const setAgentAccess = vi.fn();
const agentLog = vi.fn();
const agentCommandPath = vi.fn();

vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('../lib/ipc', () => ({
  agentAccessEnabled: () => agentAccessEnabled(),
  setAgentAccess: (on: boolean) => setAgentAccess(on),
  agentLog: () => agentLog(),
  agentCommandPath: () => agentCommandPath(),
}));

const PATH = '/Apps/Synthetic.app/Contents/MacOS/mnema';
const entry = (over: Partial<AgentLogEntry>): AgentLogEntry => ({ at: '2026-01-02T03:04:05Z', op: 'search', ...over });

beforeEach(() => {
  setLocale('en');
  agentAccessEnabled.mockReset().mockResolvedValue(false);
  setAgentAccess.mockReset().mockResolvedValue(true);
  agentLog.mockReset().mockResolvedValue([]);
  agentCommandPath.mockReset().mockResolvedValue(PATH);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

test('access is off by default and no command is shown', async () => {
  render(Agents);
  const box = screen.getByTestId('agents-access') as HTMLInputElement;
  await waitFor(() => expect(agentAccessEnabled).toHaveBeenCalled());
  expect(box.checked).toBe(false);
  expect(screen.queryByTestId('agents-cmd')).toBeNull();
  expect(screen.queryByTestId('agents-json')).toBeNull();
});

test('turning access on calls set_agent_access with true and shows the command with the real path', async () => {
  render(Agents);
  agentAccessEnabled.mockResolvedValue(true);
  await fireEvent.click(screen.getByTestId('agents-access'));
  expect(setAgentAccess).toHaveBeenCalledWith(true);
  await waitFor(() => expect(screen.getByTestId('agents-cmd').textContent).toBe(`claude mcp add mnema -- "${PATH}" mcp`));
  expect(screen.getByTestId('agents-json').textContent)
    .toBe(`{"mcpServers":{"mnema":{"command":"${PATH}","args":["mcp"]}}}`);
});

test('a refused start shows the error and does not claim access is on', async () => {
  setAgentAccess.mockRejectedValue('Cannot bind the agent socket');
  render(Agents);
  await fireEvent.click(screen.getByTestId('agents-access'));
  await waitFor(() => expect(screen.getByTestId('agents-failure').textContent).toContain('Cannot bind the agent socket'));
  expect((screen.getByTestId('agents-access') as HTMLInputElement).checked).toBe(false);
  expect(screen.queryByTestId('agents-cmd')).toBeNull();
});

test('the copy buttons put the command and the fragment on the clipboard', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  agentAccessEnabled.mockResolvedValue(true);
  render(Agents);
  await waitFor(() => expect(screen.getByTestId('agents-cmd').textContent).toContain(PATH));
  await fireEvent.click(screen.getByTestId('agents-copy-cmd'));
  await fireEvent.click(screen.getByTestId('agents-copy-json'));
  expect(writeText).toHaveBeenNthCalledWith(1, `claude mcp add mnema -- "${PATH}" mcp`);
  expect(writeText).toHaveBeenNthCalledWith(2, `{"mcpServers":{"mnema":{"command":"${PATH}","args":["mcp"]}}}`);
});

test('the log lists entries newest first with the claimed client name', async () => {
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

test('an empty log says no agent has asked yet', async () => {
  render(Agents);
  expect(await screen.findByText('No agent has asked yet')).toBeTruthy();
});

test('the log is polled every two seconds while mounted, and no longer after', async () => {
  vi.useFakeTimers();
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
