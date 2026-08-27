import { afterEach, describe, expect, it } from 'vitest';
import { formatIndexedAt } from './recency';
import { setLocale } from './index';

// `locale` is a module-level store shared by every test in this file; restore
// unconditionally, the way the other i18n suites do.
afterEach(() => setLocale('en'));

// A whole number of seconds, so `Math.floor(nowMs / 1000)` has nothing to round
// and every case below is exactly the delta it says it is.
const NOW_MS = 1_700_000_000_000;
const at = (secondsAgo: number) => 1_700_000_000 - secondsAgo;
const ago = (secondsAgo: number) => formatIndexedAt(at(secondsAgo), NOW_MS);

describe('formatIndexedAt', () => {
  // 🔴 The unit trap, and the reason the boundaries are pinned one second either
  // side rather than in the middle of each band. `indexedAt` is SECONDS
  // (`schema.sql:261`, `unixepoch()`) and `Date.now()` is milliseconds: an
  // implementation that subtracted them directly would render `19675 days ago`
  // for a document indexed a minute ago — a plausible sentence, and wrong by a
  // factor of a thousand. Every case here would redden.
  it('crosses each unit boundary where it says it does', () => {
    setLocale('en');
    expect(ago(0)).toBe('just now');
    expect(ago(59)).toBe('just now');
    expect(ago(60)).toBe('1 minute ago');
    expect(ago(119)).toBe('1 minute ago'); // floored, not rounded up
    expect(ago(3599)).toBe('59 minutes ago');
    expect(ago(3600)).toBe('1 hour ago');
    expect(ago(86_399)).toBe('23 hours ago');
    expect(ago(86_400)).toBe('1 day ago');
  });

  // A clock change, or a machine whose zone moved, can hand this card a
  // timestamp in the future. `-30 s` must not read as `-1 minutes ago` or as
  // `0 minutes ago`; under a minute in either direction is "just now".
  it('reads a timestamp from the future as just now, never as a negative count', () => {
    setLocale('en');
    expect(formatIndexedAt(at(-30), NOW_MS)).toBe('just now');
    expect(formatIndexedAt(at(-86_400), NOW_MS)).toBe('just now');
  });

  // Nothing caps the day count, and that is the decision rather than an
  // oversight: the Recents list is what was indexed LAST, so a large number
  // means nothing has been indexed since, and the sentence is still true.
  it('does not cap the day count', () => {
    setLocale('en');
    expect(ago(86_400 * 412)).toBe('412 days ago');
  });

  // The arms, pinned the way `i18n.test.ts` pins the catalogue's other plurals:
  // with counts, which need no fixture. Ukrainian takes the accusative after
  // «тому», and the teen exception is the case a hand-written rule gets wrong.
  it('applies the Ukrainian plural arms, including the teen exception', () => {
    setLocale('uk');
    expect(ago(0)).toBe('щойно');
    expect(ago(60)).toBe('1 хвилину тому');
    expect(ago(120)).toBe('2 хвилини тому');
    expect(ago(300)).toBe('5 хвилин тому');
    expect(ago(3600)).toBe('1 годину тому');
    expect(ago(2 * 3600)).toBe('2 години тому');
    expect(ago(5 * 3600)).toBe('5 годин тому');
    expect(ago(86_400)).toBe('1 день тому');
    expect(ago(2 * 86_400)).toBe('2 дні тому');
    expect(ago(5 * 86_400)).toBe('5 днів тому');
    expect(ago(11 * 86_400)).toBe('11 днів тому'); // teen → many
    expect(ago(21 * 86_400)).toBe('21 день тому');
  });

  it('applies the English arms in both directions', () => {
    setLocale('en');
    expect(ago(2 * 60)).toBe('2 minutes ago');
    expect(ago(2 * 3600)).toBe('2 hours ago');
    expect(ago(2 * 86_400)).toBe('2 days ago');
  });
});
