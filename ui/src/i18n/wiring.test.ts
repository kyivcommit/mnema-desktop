import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { setLocale, syncDocument } from './index';
import Launcher from '../launcher/Launcher.svelte';

// Launcher.svelte reads getCurrentWebviewWindow() at module scope (line 7); outside a Tauri
// webview that throws, so rendering it here needs the same mock Launcher.test.ts uses.
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ hide: vi.fn() }),
}));

// Launcher now calls model_settings() on mount (Task 7's arms-row seed); there is no
// global setupFiles mock for @tauri-apps/api/core (vite.config.ts test block), so
// without this the real invoke would run here.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: () => Promise.resolve({ key: { kind: 'absent' }, index: { kind: 'read', embeddingModel: null, searchTextArm: true, searchContentArm: false } }),
}));

describe('i18n wiring', () => {
  it('keeps input state across a locale switch', async () => {
    const { getByRole, container } = render(Launcher);
    const input = getByRole('textbox') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'draft query' } });
    setLocale('uk'); // a REAL switch away from the 'en' module default — 'en' would be a no-op
    await tick();
    expect(input.value).toBe('draft query'); // no remount
    // the switch actually took effect, so "no remount" isn't just "nothing happened"
    expect(container.querySelector('[aria-label]')!.getAttribute('aria-label')).toContain('Пін');
  });

  it('switches the pin label UK↔EN (spec §7)', async () => {
    const { container } = render(Launcher);
    const label = () => container.querySelector('[aria-label]')!.getAttribute('aria-label')!;
    setLocale('uk'); await tick(); expect(label()).toContain('Пін');
    setLocale('en'); await tick(); expect(label()).toContain('Pin');
  });

  it('syncs document.documentElement.lang to the locale', () => {
    syncDocument('uk');
    expect(document.documentElement.lang).toBe('uk');
  });

  it('the module-level locale subscription — not just syncDocument itself — drives document.lang', () => {
    // Reset first: a 'uk' left over from another test must not paper over broken wiring here.
    document.documentElement.lang = '';
    setLocale('uk');
    expect(document.documentElement.lang).toBe('uk');
  });
});
