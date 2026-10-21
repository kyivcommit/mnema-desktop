import { afterEach, describe, expect, it } from 'vitest';
import { formatDuration, formatIndexedAt, formatIndexedDate, dayBucket } from './recency';
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

// ---------------------------------------------------------------------------
// `formatIndexedDate` — D-e's other half. The relative phrase above answers
// "how long ago"; §9.3 asks for «останнє оновлення з датою», and a date is what
// a person compares against the file they edited this morning. It takes its
// locale as an ARGUMENT rather than reading the store, so the caller passes
// `$locale` and the value re-derives on a language switch like every other
// string on that screen.
// ---------------------------------------------------------------------------
describe('formatIndexedDate', () => {
  // A fixed instant, and the expectation COMPUTED here rather than written out.
  // The test machine's time zone is not the CI machine's, and a literal would
  // pin whichever zone the author happened to be in — the section deliberately
  // formats in the machine's own zone, because the person reading it is on that
  // machine.
  const AT = 1_700_000_000; // seconds, the unit `lastIndexedAt` arrives in
  const expected = (loc: string) =>
    new Intl.DateTimeFormat(loc, { dateStyle: 'long', timeStyle: 'short' }).format(new Date(AT * 1000));

  it('formats the timestamp as a date and time in the locale it is given', () => {
    expect(formatIndexedDate(AT, 'uk')).toBe(expected('uk'));
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
  });

  // Both directions, and this is the assertion that says the argument is READ.
  // A body that ignored its `locale` and formatted in one fixed language would
  // satisfy one of the two cases above and every "is a non-empty string" check
  // anyone would think to add.
  it('gives two different strings for the two locales', () => {
    expect(formatIndexedDate(AT, 'uk')).not.toBe(formatIndexedDate(AT, 'en'));
  });

  // The seconds/milliseconds trap `formatIndexedAt`'s own header names, in the
  // form this function can make it: `Date` takes milliseconds, `lastIndexedAt`
  // is seconds, and a body that passed the number through unmultiplied would
  // render a date in January 1970 for every index ever built.
  it('reads its argument as seconds, not as milliseconds', () => {
    const asMilliseconds = new Intl.DateTimeFormat('en', { dateStyle: 'long', timeStyle: 'short' }).format(new Date(AT));
    expect(formatIndexedDate(AT, 'en')).not.toBe(asMilliseconds);
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
  });

  // F1 (measured live, 2026-09-04; superseded 2026-09-10 — see the function's
  // own comment): `Intl.DateTimeFormat('uk', {dateStyle: 'long'})` used to end
  // its own string in «р.», adjacent to `indexing_index_updated`'s own full
  // stop. Once the time follows the date, «р.» sits ahead of the time rather
  // than at the end of the string — measured here, against the SAME call the
  // function makes, rather than assumed: neither locale's combined form ends
  // in a stop, because a `timeStyle: 'short'` value never does.
  it('carries no trailing stop in either locale, because a short time never ends in one', () => {
    expect(formatIndexedDate(AT, 'uk').endsWith('.')).toBe(false);
    expect(formatIndexedDate(AT, 'en').endsWith('.')).toBe(false);
    expect(expected('uk').endsWith('.')).toBe(false);
    expect(expected('en').endsWith('.')).toBe(false);
    expect(formatIndexedDate(AT, 'uk')).toBe(expected('uk'));
    expect(formatIndexedDate(AT, 'en')).toBe(expected('en'));
  });
});

// ---------------------------------------------------------------------------
// `dayBucket` — the Recents panel's date groups. Local calendar days, built
// here from local date parts so the expectations hold in any zone: a literal
// UTC stamp would only be right in the zone its author sat in.
// ---------------------------------------------------------------------------
describe('dayBucket', () => {
  const NOW = new Date(2026, 9, 21, 12, 0, 0).getTime();
  // `askedAt` is SECONDS (`unixepoch()`), `now` milliseconds.
  const on = (year: number, month: number, day: number, hour = 12) =>
    new Date(year, month, day, hour, 0, 0).getTime() / 1000;

  // 🔴 The boundary is LOCAL MIDNIGHT, not 24 elapsed hours: read at 00:30, a
  // question asked at 23:45 is yesterday, and an implementation computing
  // `now - asked < 24 h` would call it today. The 00:00 and 23:00 rows below
  // are the two sides of that line.
  it('puts a question on the local day it was asked, not on an elapsed span', () => {
    expect(dayBucket(on(2026, 9, 21, 0), NOW)).toBe('today');
    expect(dayBucket(on(2026, 9, 21, 11), NOW)).toBe('today');
    expect(dayBucket(on(2026, 9, 20, 23), NOW)).toBe('yesterday');
    expect(dayBucket(on(2026, 9, 20, 12), NOW)).toBe('yesterday');
  });

  it('puts days 2 to 7 in the week and anything older in earlier', () => {
    expect(dayBucket(on(2026, 9, 19), NOW)).toBe('week'); // 2
    expect(dayBucket(on(2026, 9, 14), NOW)).toBe('week'); // 7, the last day of the band
    expect(dayBucket(on(2026, 9, 13), NOW)).toBe('earlier'); // 8
    expect(dayBucket(on(2026, 8, 21), NOW)).toBe('earlier'); // a month back
  });

  // A clock stepped back, or a machine whose zone moved, can hand this a stamp
  // from the future. It must not fall through to a bucket that does not exist.
  it('reads a stamp from the future as today', () => {
    expect(dayBucket(on(2026, 9, 21, 18), NOW)).toBe('today');
    expect(dayBucket(on(2026, 9, 25), NOW)).toBe('today');
  });

  // Month and year boundaries are day arithmetic, not "same month" or "same
  // year": a wrong implementation would put these in the wrong band.
  it('counts across a month and a year boundary', () => {
    const newYear = new Date(2027, 0, 2, 12, 0, 0).getTime();
    expect(dayBucket(on(2026, 11, 31), newYear)).toBe('week'); // 2 days
    expect(dayBucket(on(2026, 11, 26), newYear)).toBe('week'); // 7 days
  });
});

describe('formatDuration', () => {
  it('reads as days, hours, minutes and seconds, dropping the zero units', () => {
    setLocale('uk');
    expect(formatDuration(0)).toBe('0 с');
    expect(formatDuration(59)).toBe('59 с');
    expect(formatDuration(2100)).toBe('35 хв');
    expect(formatDuration(2130)).toBe('35 хв 30 с');
    expect(formatDuration(90_061)).toBe('1 д 1 год 1 хв 1 с');
    expect(formatDuration(86_400 + 5)).toBe('1 д 5 с');
    setLocale('en');
    expect(formatDuration(2130)).toBe('35 min 30 s');
  });
});
