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

describe('i18n wiring', () => {
  it('keeps input state across a locale switch', async () => {
    const { getByRole } = render(Launcher);
    const input = getByRole('textbox') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'draft query' } });
    setLocale('en');
    expect(input.value).toBe('draft query'); // no remount
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
});
