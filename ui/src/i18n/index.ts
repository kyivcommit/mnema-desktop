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
