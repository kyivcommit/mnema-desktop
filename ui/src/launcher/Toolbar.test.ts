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
// The panel toggles live on `Shell` (§2); this cluster is the cloud and the
// settings button.
const props = (over: Record<string, unknown> = {}) => ({ status: OK, ...over }) as never;
const buttons = () => screen.getAllByRole('button');

test('the buttons stand in the order cloud, settings', () => {
  setLocale('en');
  render(Toolbar, props());
  expect(buttons().map((b) => b.dataset.testid)).toEqual(['provider-cloud', 'settings']);
});

test('every button has a non-empty title equal to its aria-label, in both languages', () => {
  const seen: string[][] = [];
  for (const loc of ['en', 'uk'] as const) {
    setLocale(loc);
    const { unmount } = render(Toolbar, props());
    expect(buttons()).toHaveLength(2); // positive control: the loop below is not over nothing
    for (const b of buttons()) {
      expect(b.getAttribute('aria-label')).toBeTruthy();
      expect(b.getAttribute('title')).toBe(b.getAttribute('aria-label'));
    }
    seen.push(buttons().map((b) => b.getAttribute('aria-label')!));
    unmount();
  }
  expect(seen[0]).not.toEqual(seen[1]);
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
  expect(title(OK)).toContain('OpenRouter');
  setLocale('uk');
  expect(title(OK)).toContain('OpenRouter');
});

test('settings opens without a section', async () => {
  render(Toolbar, props());
  await fireEvent.click(screen.getByTestId('settings'));
  expect(openSettings).toHaveBeenCalledWith();
});

test('the cloud names the missing local models, not the key', () => {
  setLocale('uk');
  render(Toolbar, props({ status: { kind: 'notConfigured', missing: 'localModels' } }));
  const cloud = screen.getByTestId('provider-cloud');
  expect(cloud.getAttribute('aria-label')).toBe('Завантажте моделі Mnema — відкрити налаштування моделей');
  expect(cloud.getAttribute('aria-label')).not.toContain('ключа');
});

test('the cloud names an empty role, not the embedding model', () => {
  setLocale('uk');
  render(Toolbar, props({ status: { kind: 'notConfigured', missing: 'role' } }));
  expect(screen.getByTestId('provider-cloud').getAttribute('aria-label')).toBe('Не обрано провайдера для ролі — відкрити налаштування моделей');
});

test('the status button names the provider in use', () => {
  const name = (over: Record<string, unknown>) => {
    const r = render(Toolbar, props(over));
    const b = screen.getByTestId('provider-cloud');
    const v = [b.getAttribute('aria-label')!, b.getAttribute('title')!];
    r.unmount();
    return v;
  };
  // Under Mnema, ready or unreachable, OpenRouter is not named.
  for (const status of [OK, { kind: 'unreachable', reason: 'refused' }] as ProviderStatus[]) {
    const [label, title] = name({ provider: 'mnema', status });
    expect(label).toContain('Mnema');
    expect(title).toContain('Mnema');
    expect(label).not.toContain('OpenRouter');
    expect(title).not.toContain('OpenRouter');
  }
  // And the other direction: OpenRouter's wording is unchanged.
  expect(name({ provider: 'openRouter' })[0]).toBe('Connected to OpenRouter');
  expect(name({})[0]).toBe('Connected to OpenRouter');
  setLocale('uk');
  expect(name({ provider: 'mnema' })[0]).toContain('Mnema');
});
