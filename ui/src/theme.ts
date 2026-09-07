import { writable } from 'svelte/store';
import type { ThemeChoice } from './lib/ipc';

// The one writer of `data-theme` on this document. Three feeders — the boot
// snapshot, the `theme-changed` event from Rust, and the settings window's own
// successful `set_theme` — all go through this store, so there is one place the
// attribute is decided and one subscription that writes it.
export const theme = writable<ThemeChoice>('system');

export function isThemeChoice(v: unknown): v is ThemeChoice {
  return v === 'system' || v === 'light' || v === 'dark';
}

// Explicit choice → the attribute; system → NO attribute. `tokens.css` reads
// `:root[data-theme="dark"]` and `:root:not([data-theme="light"])` under
// `prefers-color-scheme: dark`, so absence IS "follow the OS". A literal
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
