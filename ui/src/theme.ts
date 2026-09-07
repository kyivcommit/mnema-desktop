import { writable } from 'svelte/store';
import { setTheme, type ThemeChoice } from './lib/ipc';

// The one writer of `data-theme` on this document. Three feeders live here:
// the boot snapshot, the `theme-changed` event (which Rust broadcasts to the
// asking window too), and `changeTheme` below, which the settings section
// calls. All three go through this store, so there is one place the attribute
// is decided and one subscription that writes it.
//
// Two different orderings keep this store honest, and they are enforced in two
// different places. That the event order equals the file order is Rust's doing
// — `theme::change_theme` holds one lock across persist, apply and broadcast.
// That an older reply cannot outrank a newer change is `changeSeq`'s doing,
// below.
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

// Module-level, and not a field of the settings section: `Settings.svelte`
// destroys and recreates that section on every navigation, so a per-component
// counter would start again at zero and a continuation left behind by a
// destroyed instance would still look current. The newest change owns the
// store; an older reply that lands after it writes nothing.
let changeSeq = 0;

// Persists `choice` and, if no newer change has started since, moves the store.
// Rejects with the backend's own sentence; nothing was written and nothing was
// applied in that case (`set_theme` persists first), so this call left the
// store alone and the caller re-reads nothing. A change that succeeded
// alongside a rejected newer one is not this call's to apply — its own
// `theme-changed` broadcast is what moves the store then.
export async function changeTheme(choice: ThemeChoice): Promise<void> {
  const mine = ++changeSeq;
  await setTheme(choice);
  if (mine === changeSeq) theme.set(choice);
}

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
