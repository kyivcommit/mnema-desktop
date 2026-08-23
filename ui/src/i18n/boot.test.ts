import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';

// bootLocale resolves the startup locale from a `get_locale` snapshot and then
// follows `locale-changed` events. These mocks let a test interleave the two —
// fire a switch DURING boot — and prove the stale snapshot cannot clobber the
// newer event (reviewer F1: the listener must be up before the snapshot).
const h = vi.hoisted(() => {
  const state: { handler: ((e: { payload: 'uk' | 'en' }) => void) | null } = { handler: null };
  const invoke = vi.fn();
  const listen = vi.fn(async (_name: string, cb: (e: { payload: 'uk' | 'en' }) => void) => {
    state.handler = cb;
    return () => {};
  });
  return { state, invoke, listen };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: h.listen }));

import { bootLocale, locale, setLocale } from './index';

beforeEach(() => {
  h.state.handler = null;
  h.invoke.mockReset();
  h.listen.mockClear(); // keep the hoisted capture impl; clear only call history
  setLocale('en');
});

describe('bootLocale ordering (reviewer F1)', () => {
  it('registers the locale-changed listener before taking the snapshot', async () => {
    const order: string[] = [];
    h.listen.mockImplementationOnce(async (_n: string, cb: (e: { payload: 'uk' | 'en' }) => void) => {
      order.push('listen');
      h.state.handler = cb;
      return () => {};
    });
    h.invoke.mockImplementationOnce(async () => {
      order.push('invoke');
      return { choice: 'auto', effective: 'en' };
    });
    await bootLocale();
    // A switch that lands during the snapshot round-trip is lost unless the
    // listener is already registered. Old code awaited invoke first.
    expect(order).toEqual(['listen', 'invoke']);
  });

  it('a switch during boot wins over the stale snapshot reply', async () => {
    let resolveSnapshot!: (v: { choice: string; effective: 'uk' | 'en' }) => void;
    h.invoke.mockImplementationOnce(
      () => new Promise((res) => { resolveSnapshot = res as typeof resolveSnapshot; }),
    );
    const boot = bootLocale();
    await vi.waitFor(() => expect(h.state.handler).toBeTruthy(), { timeout: 500, interval: 5 });
    h.state.handler!({ payload: 'uk' }); // a live switch lands during boot
    resolveSnapshot({ choice: 'auto', effective: 'en' }); // the stale snapshot arrives after
    await boot;
    expect(get(locale)).toBe('uk'); // the snapshot must not overwrite the newer event
  });

  it('applies the snapshot when no switch happens during boot (the common case)', async () => {
    h.invoke.mockImplementationOnce(async () => ({ choice: 'uk', effective: 'uk' as const }));
    await bootLocale();
    expect(get(locale)).toBe('uk');
  });
});
