import { t, type Loc } from './index';

// How long ago a document was indexed, for the Recents tab (review Minor 5).
//
// 🔴 The unit is what this file exists to get right. `RecentDoc.indexedAt`
// (`lib/ipc.ts:65`) is `ingest_stage.updated_at` (`mnema-index/src/write.rs:286`),
// an `INTEGER … DEFAULT (unixepoch())` column (`crates/mnema-index/src/schema.sql:261`)
// — SECONDS since the epoch, where `Date.now()` is milliseconds. A comparison
// that mixed the two would be wrong by a factor of a thousand and would still
// render a plausible sentence.
//
// Relative rather than a formatted date, and that is a decision: a date needs a
// time zone, so what a person sees would depend on the machine the card runs on
// and on which zone a test happens to run in, while "how long ago" is the
// question the card's own name asks and needs no zone at all. The cost is that
// an old entry reads as `412 days ago` rather than as a date; the Recents list
// is what was indexed LAST, so that is a state the person has arrived at by
// having indexed nothing since, and the sentence is still true.
const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * `indexedAt` in seconds since the epoch, `nowMs` in milliseconds (`Date.now()`).
 * Anything under a minute — including a timestamp in the FUTURE, which a clock
 * change or a machine whose zone moved can produce — reads as "just now" rather
 * than as a negative count.
 *
 * ⚠️ This sentence ages on screen, and that is a property the rest of the card
 * does not have. `Tree.svelte`'s refresh deliberately keeps the listing that
 * worked when a refresh FAILS — a stale *row* was true of the past and probably
 * still is, so keeping it is right. A stale *"1 minute ago"* is different: it is
 * an assertion about **now**, and two hours after a failed refresh it is simply
 * false. Bounded by the next successful refresh, and every alternative (blank
 * the label, blank the card) is worse — so it is accepted, not overlooked.
 */
export function formatIndexedAt(indexedAt: number, nowMs: number): string {
  const delta = Math.floor(nowMs / 1000) - indexedAt;
  if (delta < MINUTE) return t('recent_now');
  if (delta < HOUR) return t('recent_minutes', { count: Math.floor(delta / MINUTE) });
  if (delta < DAY) return t('recent_hours', { count: Math.floor(delta / HOUR) });
  return t('recent_days', { count: Math.floor(delta / DAY) });
}

/**
 * The same instant as a DATE, for §9.3's «останнє оновлення з датою» (D-e).
 *
 * It lives beside `formatIndexedAt` so one module still answers for one kind of
 * time, and it is not a duplicate of it: «3 дні тому» is what a person feels,
 * the date is what they compare against the file they edited this morning. The
 * Recents card's argument for a relative phrase ONLY (the header above) held
 * because that card has no zone to be right about; §9.3 does — it is a person
 * reading their own index on their own machine, so the machine's own zone is
 * the right one and `Intl.DateTimeFormat` is left to take it from the runtime.
 *
 * 🔴 `indexedAt` is SECONDS, the same unit and the same trap as above:
 * `MAX(ingest_stage.updated_at)` is an `INTEGER … DEFAULT (unixepoch())` column
 * (`crates/mnema-index/src/schema.sql:261`), and `Date` takes milliseconds. A
 * body that passed the number straight through would render a date in January
 * 1970 for every index ever built, and it would look like a date.
 *
 * The locale is an ARGUMENT rather than a read of the store, so the caller
 * writes `formatIndexedDate(at, $locale)` inside its `$derived.by` and the
 * string re-derives on a language switch — the same anchoring every other
 * reactive string on that screen needs.
 */
export function formatIndexedDate(indexedAt: number, locale: Loc): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'long' }).format(new Date(indexedAt * 1000));
}
