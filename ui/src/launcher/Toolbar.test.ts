import { render, screen, fireEvent } from '@testing-library/svelte';
import { vi, expect, test, beforeEach, afterEach } from 'vitest';
import Toolbar from './Toolbar.svelte';
import { setLocale } from '../i18n';
import type { ProviderStatus } from '../lib/ipc';

const openSettings = vi.fn();
vi.mock('../lib/ipc', () => ({ openSettings: (...a: unknown[]) => openSettings(...a) }));
beforeEach(() => openSettings.mockReset().mockResolvedValue(undefined));
afterEach(() => setLocale('en'));

const OK: ProviderStatus = { kind: 'ok' };
const props = (over: Record<string, unknown> = {}) =>
  ({ heat: 'hot', left: true, right: true, pinned: false, status: OK, ...over }) as never;
const buttons = () => screen.getAllByRole('button');

test('the buttons stand in the order cloud, left, right, pin, settings', () => {
  setLocale('en');
  render(Toolbar, props());
  expect(buttons().map((b) => b.dataset.testid)).toEqual(['provider-cloud', 'toggle-left', 'toggle-right', 'pin', 'settings']);
});

test('every button has a non-empty title equal to its aria-label, in both languages', () => {
  const seen: string[][] = [];
  for (const loc of ['en', 'uk'] as const) {
    setLocale(loc);
    const { unmount } = render(Toolbar, props());
    expect(buttons()).toHaveLength(5); // positive control: the loop below is not over nothing
    for (const b of buttons()) {
      expect(b.getAttribute('aria-label')).toBeTruthy();
      expect(b.getAttribute('title')).toBe(b.getAttribute('aria-label'));
    }
    seen.push(buttons().map((b) => b.getAttribute('aria-label')!));
    unmount();
  }
  expect(seen[0]).not.toEqual(seen[1]);
});

test('cold: the panel toggles are disabled and not pressed', () => {
  render(Toolbar, props({ heat: 'cold', left: false, right: false }));
  for (const id of ['toggle-left', 'toggle-right']) {
    const b = screen.getByTestId(id) as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    expect(b.getAttribute('aria-pressed')).toBe('false');
  }
});

test('hot: a click flips the toggle', async () => {
  render(Toolbar, props());
  const left = screen.getByTestId('toggle-left');
  expect(left.getAttribute('aria-pressed')).toBe('true');
  await fireEvent.click(left);
  expect(left.getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByTestId('toggle-right').getAttribute('aria-pressed')).toBe('true');
  await fireEvent.click(left);
  expect(left.getAttribute('aria-pressed')).toBe('true');
});

test('the pin flips on click', async () => {
  render(Toolbar, props());
  const pin = screen.getByTestId('pin');
  await fireEvent.click(pin);
  expect(pin.getAttribute('aria-pressed')).toBe('true');
});

test('the cloud opens the models section only when nothing is configured', async () => {
  const { unmount } = render(Toolbar, props({ status: { kind: 'notConfigured', missing: 'key' } }));
  await fireEvent.click(screen.getByTestId('provider-cloud'));
  expect(openSettings).toHaveBeenCalledTimes(1);
  expect(openSettings).toHaveBeenCalledWith('models');
  unmount();
  openSettings.mockClear();
  for (const status of [{ kind: 'unreachable', reason: 'timeout' }, OK]) {
    const r = render(Toolbar, props({ status }));
    await fireEvent.click(screen.getByTestId('provider-cloud'));
    r.unmount();
  }
  expect(openSettings).not.toHaveBeenCalled();
});

test('the cloud title says what is missing, and why it is unreachable', () => {
  const title = (status: ProviderStatus) => {
    const r = render(Toolbar, props({ status }));
    const v = screen.getByTestId('provider-cloud').getAttribute('title')!;
    r.unmount();
    return v;
  };
  const key = title({ kind: 'notConfigured', missing: 'key' });
  const model = title({ kind: 'notConfigured', missing: 'embeddingModel' });
  expect(key).toBeTruthy();
  expect(model).toBeTruthy();
  expect(key).not.toBe(model);
  expect(title({ kind: 'unreachable', reason: 'connection refused' })).toContain('connection refused');
  expect(title(OK)).not.toBe(key);
});

test('settings opens without a section', async () => {
  render(Toolbar, props());
  await fireEvent.click(screen.getByTestId('settings'));
  expect(openSettings).toHaveBeenCalledWith();
});
