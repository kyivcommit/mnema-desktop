import { writable, get } from 'svelte/store';
import { IntlMessageFormat } from 'intl-messageformat';
import { messages, type Key } from './catalog';

export type Loc = 'uk' | 'en';
export const locale = writable<Loc>('en');
const FALLBACK: Loc = 'en';
const cache = new Map<string, IntlMessageFormat>();

export function t(key: Key, values?: Record<string, unknown>): string {
  const loc = get(locale);
  const msg = messages[loc][key] ?? messages[FALLBACK][key];
  if (msg === undefined) return messages[FALLBACK][key] ?? messages.en.pin; // never a raw key
  const ck = `${loc}:${key}`;
  let f = cache.get(ck);
  if (!f) { f = new IntlMessageFormat(msg, loc); cache.set(ck, f); }
  return String(f.format(values));
}

export function setLocale(l: Loc) { locale.set(l); }
export function initLocale(effective: Loc) { setLocale(effective); }

export function syncDocument(l: Loc) {
  document.documentElement.lang = l; // accessibility: document language follows the locale
}
locale.subscribe((l) => syncDocument(l)); // reactive, no component remount
// document.title is per-window: launcher main.ts sets `document.title = 'Mnema'`; settings main.ts
// runs `locale.subscribe(() => document.title = 'Mnema — ' + t('settings_title'))` so the HTML
// <title> tracks the locale too (mirrors the native OS-title set from Rust).

export async function bootLocale() {
  const { invoke } = await import('@tauri-apps/api/core');
  const { listen } = await import('@tauri-apps/api/event');
  // Register the listener BEFORE taking the startup snapshot. A language switch
  // that lands during boot would otherwise fall in the gap between the snapshot
  // reply and a later listen(), and be lost until the next switch — leaving two
  // windows able to disagree (reviewer F1). A live event is always newer than
  // the snapshot, so once one has arrived it must win: the snapshot only seeds
  // the locale when no switch happened during boot.
  let liveEventSeen = false;
  await listen<'uk' | 'en'>('locale-changed', (e) => {
    liveEventSeen = true;
    setLocale(e.payload);
  });
  const reply = await invoke<{ choice: string; effective: 'uk' | 'en' }>('get_locale');
  if (!liveEventSeen) initLocale(reply.effective);
}
