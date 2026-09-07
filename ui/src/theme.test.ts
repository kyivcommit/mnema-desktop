import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';

// The same interleaving `i18n/boot.test.ts` drives for the locale: a switch
// that lands DURING boot must beat the stale snapshot, which needs the listener
// up before the snapshot is taken.
const h = vi.hoisted(() => {
  const state: { handler: ((e: { payload: string }) => void) | null } = { handler: null };
  const invoke = vi.fn();
  const listen = vi.fn(async (_name: string, cb: (e: { payload: string }) => void) => {
    state.handler = cb;
    return () => {};
  });
  return { state, invoke, listen };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: h.listen }));

import { bootTheme, theme, applyTheme } from './theme';

const attribute = () => document.documentElement.dataset.theme;

beforeEach(() => {
  h.state.handler = null;
  h.invoke.mockReset();
  h.listen.mockClear();
  theme.set('system');
});

describe('bootTheme ordering', () => {
  it('registers the theme-changed listener before taking the snapshot', async () => {
    const order: string[] = [];
    h.listen.mockImplementationOnce(async (_n: string, cb: (e: { payload: string }) => void) => {
      order.push('listen');
      h.state.handler = cb;
      return () => {};
    });
    h.invoke.mockImplementationOnce(async () => {
      order.push('invoke');
      return { choice: 'system' };
    });
    await bootTheme();
    expect(order).toEqual(['listen', 'invoke']);
    expect(h.listen).toHaveBeenCalledWith('theme-changed', expect.any(Function));
    expect(h.invoke).toHaveBeenCalledWith('get_theme');
  });

  it('a switch during boot wins over the stale snapshot reply', async () => {
    let resolveSnapshot!: (v: { choice: string }) => void;
    h.invoke.mockImplementationOnce(
      () => new Promise((res) => { resolveSnapshot = res as typeof resolveSnapshot; }),
    );
    const boot = bootTheme();
    await vi.waitFor(() => expect(h.state.handler).toBeTruthy(), { timeout: 500, interval: 5 });
    h.state.handler!({ payload: 'dark' });
    resolveSnapshot({ choice: 'light' });
    await boot;
    expect(get(theme)).toBe('dark');
    expect(attribute()).toBe('dark');
  });

  it('applies the snapshot when no switch happens during boot, and the document follows', async () => {
    h.invoke.mockImplementationOnce(async () => ({ choice: 'light' }));
    await bootTheme();
    expect(get(theme)).toBe('light');
    expect(attribute()).toBe('light');
  });
});

describe('the document attribute', () => {
  it('an explicit choice sets data-theme and the system choice removes it, not renames it', () => {
    applyTheme('dark');
    expect(attribute()).toBe('dark');
    applyTheme('system');
    // Absence, not "system": `tokens.css` matches `[data-theme="dark"]` and
    // `:not([data-theme="light"])`; a literal "system" would behave like
    // absence today and stop the day a selector matches `[data-theme]`.
    expect(attribute()).toBeUndefined();
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
  });

  it('the module-level subscription drives the document, not only applyTheme itself', () => {
    theme.set('light');
    expect(attribute()).toBe('light');
    theme.set('system');
    expect(attribute()).toBeUndefined();
  });

  it('an unknown value on the wire is read as system, from the snapshot and from an event alike', async () => {
    theme.set('dark');
    h.invoke.mockImplementationOnce(async () => ({ choice: 'sepia' }));
    await bootTheme();
    expect(get(theme)).toBe('system');
    expect(attribute()).toBeUndefined();

    theme.set('dark');
    h.state.handler!({ payload: 'sepia' });
    expect(get(theme)).toBe('system');
    expect(attribute()).toBeUndefined();
  });
});
