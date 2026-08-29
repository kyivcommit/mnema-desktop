import { afterEach, describe, it, expect, vi } from 'vitest';
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
//
// 🔴 Answered PER COMMAND, not one object for all of them. These tests render
// state A, where no card draws and `list_tree` is never called — so a blanket
// answer is green today for a reason that has nothing to do with these tests,
// and one state further along `Tree` reads `.roots` off the settings object and
// throws. That is the same trap Ruling Y closed in `Launcher.test.ts` and the
// in-flight draft test closed again; this is its third home.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string) =>
    cmd === 'list_tree'
      ? Promise.resolve({ roots: [], recents: [] })
      : Promise.resolve({ key: { kind: 'absent' }, index: { kind: 'read', embeddedChunks: 0, embeddedChunksEverywhere: 0, embeddingModel: null, searchTextArm: true, searchContentArm: false } }),
}));

// 🔴 U1, booked forward from Task 6 and now paid. Both tests below used to read
// `container.querySelector('[aria-label]')` and ASSUME the first labelled element
// was the pin. That was true only because state A draws no labelled card — a
// neighbour's defence, not their own. Task 8b made a labelled card render in one
// more state and ruling I-B in five, and Task 9 adds another; under any of those
// the tree card's label wins the selector and these tests fail with
// `expected 'Дерево' to contain 'Пін'`, which says nothing about the pin.
// `data-testid="pin"` is stable across locales AND across whatever cards render,
// which the accessible name (the very thing under test) cannot be.
const pinLabel = (container: HTMLElement) =>
  container.querySelector('[data-testid="pin"]')!.getAttribute('aria-label')!;

// `locale` is a module-level store shared by every test here, and the two pin
// tests switch it. An in-test restore is skipped when an assertion fails first,
// which leaves the last test in this file — whose whole claim is that
// `setLocale('uk')` is a REAL switch — passing or failing for a reason that has
// nothing to do with it. Restore unconditionally, as every other suite does.
afterEach(() => setLocale('en'));

describe('i18n wiring', () => {
  it('keeps input state across a locale switch', async () => {
    const { getByRole, container } = render(Launcher);
    const input = getByRole('textbox') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'draft query' } });
    setLocale('uk'); // a REAL switch away from the 'en' module default — 'en' would be a no-op
    await tick();
    expect(input.value).toBe('draft query'); // no remount
    // the switch actually took effect, so "no remount" isn't just "nothing happened"
    expect(pinLabel(container)).toContain('Пін');
  });

  it('switches the pin label UK↔EN (spec §7)', async () => {
    const { container } = render(Launcher);
    setLocale('uk'); await tick(); expect(pinLabel(container)).toContain('Пін');
    setLocale('en'); await tick(); expect(pinLabel(container)).toContain('Pin');
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
