import { writable } from 'svelte/store';
import type { ThemeChoice } from './lib/ipc';

// The one writer of `data-theme` on this document. Two feeders live here —
// the boot snapshot and the `theme-changed` event, which Rust broadcasts to
// the asking window too; the settings section adds a third by setting the
// store after a successful `set_theme` reply (PR 10b, Task 3). All three go
// through this store, so there is one place the attribute is decided and one
// subscription that writes it.
export const theme = writable<ThemeChoice>('system');

export function isThemeChoice(v: unknown): v is ThemeChoice {
  return v === 'system' || v === 'light' || v === 'dark';
}

// Explicit choice → the attribute; system → NO attribute. `tokens.css` reads
// `:root[data-theme="dark"]` on its own, and `:root:not([data-theme="light"])`
// under `prefers-color-scheme: dark`, so absence IS "follow the OS". A literal
// "system" value would behave the same today and stop the day a stylesheet
// matches `[data-theme]` at all — so it is deleted, not renamed.
export function applyTheme(choice: ThemeChoice) {
  if (choice === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = choice;
}
theme.subscribe(applyTheme);

// Mirrors `bootLocale` (`i18n/index.ts`), ordering included: the listener goes
// up BEFORE the snapshot is taken, and a live event that lands during boot is
// newer than the snapshot and wins. Non-fatal by contract: both `main.ts`
// files `.catch` it, and the store's default (`system`) is the right answer for
// a window that could not ask.
export async function bootTheme() {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');
  let liveEventSeen = false;
  await listen<string>('theme-changed', (e) => {
    liveEventSeen = true;
    theme.set(isThemeChoice(e.payload) ? e.payload : 'system');
  });
  const reply = await invoke<{ choice: string }>('get_theme');
  if (!liveEventSeen) theme.set(isThemeChoice(reply.choice) ? reply.choice : 'system');
}
