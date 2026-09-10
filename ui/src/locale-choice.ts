import { writable, get, type Readable } from 'svelte/store';
import { getLocale, setLocaleChoice, type LocaleApplyError, type LocaleChoice, type LocaleReply } from './lib/ipc';
import { setLocale as applyEffectiveLocale } from './i18n';

// PR 10f (Task 3). The language selection, moved out of `Application.svelte`'s
// own state into a MODULE-level store — the same shape `./theme` already
// takes, and for the identical reason: `Settings.svelte` destroys and
// recreates `Application` on every section switch
// (`{#if section === 'application'}`, unchanged by this task), so anything
// that must survive that remount — busy, a partial-apply warning, the last
// confirmed choice — cannot live in the component that remount throws away.
//
// A read (`loadLocaleChoice`) only ever confirms `snapshot`: `get_locale`
// carries no `applyErrors`, so it has no basis to say a change applied
// everywhere or didn't, and must not clear a `partial`/`unknown` a previous
// change earned. Only `changeLocaleChoice`/`retryLocaleApplication` — which DO
// see `applyErrors` — decide `application`.
//
// 🔴 `initial` and `unknown` are deliberately two different values, not one.
// Collapsing them made a fresh, ordinary boot indistinguishable from a
// genuinely uncertain apply outcome — a person opening Settings for the first
// time in a session would read "the applied state could not be confirmed"
// under a select that had just loaded cleanly, before touching anything. Only
// a REJECTED `changeLocaleChoice`/`retryLocaleApplication`, or a read that
// itself fails, ever produces `unknown`; nothing sets it merely because
// nothing has happened yet.
export type LocaleApplicationState =
  | { kind: 'initial' } // nothing attempted this session — no opinion, and nothing to say about it
  | { kind: 'applied' } // the last confirmed change reported every surface picked it up
  | { kind: 'partial'; errors: LocaleApplyError[] } // the last confirmed change named surfaces that did not
  | { kind: 'unknown' }; // a change or a read could not be confirmed

export type LocaleChoiceState = {
  snapshot: LocaleReply | null;
  // Review, PR44 P2-2: `snapshot` alone is not "safe to retry-apply with" —
  // it is also the LAST value a read confirmed, which a change in flight (or
  // one that just rejected, with its own recovery read ALSO rejecting) can
  // leave stale and unconfirmed. Scenario the review names: a read confirms
  // `uk`; a change to `en` persists on the backend but its own reply is lost
  // in transport, and the recovery read that follows also fails — `snapshot`
  // still holds the pre-write `uk`, `application` is `unknown`, and retrying
  // with `snapshot.choice` would call `set_locale('uk')`, rolling back the
  // `en` the backend already saved. `snapshotConfirmed` is false exactly
  // there: set false the instant a change STARTS (the snapshot behind it is
  // stale from that point, whichever way the change turns out), true only by
  // a read or a command reply that itself confirms a fresh one.
  //
  // Deliberately NOT `snapshot: null` on the rejection path instead (the
  // brief's other option): the select must keep showing the last
  // AUTHORITATIVE value while unconfirmed (`languageChoice` in
  // `Application.svelte` still reads `snapshot`), not blank out to nothing
  // — only retry-apply's own eligibility is what turns out to be stale.
  snapshotConfirmed: boolean;
  busy: boolean;
  // `error` and `changeError` are two DIFFERENT operations' own rejection
  // messages, kept in two fields on purpose (review round 1, Minor 3). A
  // rejected `changeLocaleChoice` used to write its message into `error` and
  // then immediately call `loadLocaleChoice()`, which either cleared it
  // (recovery read succeeds — the change's own message vanishes with nothing
  // ever having shown it) or overwrote it with the READ's message (recovery
  // read fails — now labelled "could not be read" under a heading that is
  // actually about the CHANGE). One field cannot answer "which operation said
  // this" once a second operation touches it; two fields need no answer.
  error: string | null; // the last loadLocaleChoice()'s own rejection message
  // The last changeLocaleChoice/retryLocaleApplication's own rejection
  // message. Review round 2, Minor C — its exit rule, stated once here
  // rather than left implicit: cleared only by a SUBSEQUENT
  // changeLocaleChoice/retryLocaleApplication call, at that call's own
  // START (regardless of how it turns out) and again, redundantly, on that
  // call's SUCCESS. `loadLocaleChoice` never clears it, successful or not —
  // the same reason it never touches `application` either: a read has no
  // `applyErrors` to confirm this message resolved one way or the other, so
  // it is not entitled to an opinion about it. It can therefore outlive a
  // successful "Retry reading" indefinitely, which is correct: nothing about
  // that read answered whether the EARLIER change applied everywhere.
  changeError: string | null;
  application: LocaleApplicationState;
};

const INITIAL: LocaleChoiceState = {
  snapshot: null, snapshotConfirmed: false, busy: false, error: null, changeError: null,
  application: { kind: 'initial' },
};

const state = writable<LocaleChoiceState>({ ...INITIAL });
// Exposed as `Readable`, not `Writable`: `loadLocaleChoice`, `changeLocaleChoice`
// and `retryLocaleApplication` below are the only three writers, and a caller
// holding the raw store could set a `snapshot`/`application` pair that no real
// IPC round-trip ever produced.
export const localeChoiceState: Readable<LocaleChoiceState> = state;

const errorMessage = (e: unknown): string => (e instanceof Error ? e.message : String(e));

// Orders reads and writes against each other, module-wide — `./theme`'s own
// `changeSeq`, generalised to cover a read too. Bumped at the start of every
// operation; a result is applied only if nothing newer has started since. The
// `busy` guards below stop two WRITES from ever overlapping, so this exists
// for the one race busy cannot: a `loadLocaleChoice()` in flight before a
// change starts, resolving after that change has already settled the store.
let opSeq = 0;

// Mount-time (and remount-time) read. A no-op while `busy`: a write already
// in flight will settle the store itself once it resolves, and starting a
// second, concurrent read here could resolve in either order against it —
// this is the "remount lands mid-write" case, not a special one.
export async function loadLocaleChoice(): Promise<void> {
  if (get(state).busy) return;
  const mine = ++opSeq;
  try {
    const reply = await getLocale();
    if (mine !== opSeq) return; // superseded by a change that started after this read did
    state.update((s) => ({ ...s, snapshot: reply, error: null, snapshotConfirmed: true }));
  } catch (e) {
    if (mine !== opSeq) return;
    state.update((s) => ({ ...s, error: errorMessage(e), application: { kind: 'unknown' } }));
  }
}

// Persists `choice` and applies it everywhere `set_locale` reaches. Not
// started while `busy` — a second choice pressed before the first settles
// would send two commands the operating system could resolve in either order.
export async function changeLocaleChoice(choice: LocaleChoice): Promise<void> {
  if (get(state).busy) return;
  // Bumped (not captured into a compared variable here, unlike
  // `loadLocaleChoice`'s `mine`) so a read already in flight becomes stale
  // the moment this change starts — see that function's own check. This
  // function needs no matching self-comparison later: the `busy` guard above
  // fully serialises writes, so nothing else can start, and therefore nothing
  // else can bump `opSeq` again, between here and either exit below. Review
  // round 1, Minor 4 — an earlier draft carried a stale-`mine` check on the
  // success path anyway; being unreachable, it was never exercised, and it
  // left `busy` set forever on the one path that could have reached it, which
  // is a latent deadlock rather than a guard.
  opSeq++;
  // Review, PR44 P2-2: the snapshot behind this change is stale from THIS
  // point on, whichever way the change turns out — a rejection (with its own
  // recovery read possibly also failing) must not leave retry-apply trusting
  // a pre-write value it never re-confirmed.
  state.update((s) => ({ ...s, busy: true, changeError: null, snapshotConfirmed: false }));
  try {
    const reply = await setLocaleChoice(choice);
    // The confirmed reply is the truth, applied through the SAME setter
    // `bootLocale` uses — never guessed from `choice`, the request this
    // window sent, which is not necessarily what a persist step wrote.
    applyEffectiveLocale(reply.effective);
    state.set({
      snapshot: { choice: reply.choice, effective: reply.effective },
      snapshotConfirmed: true,
      busy: false,
      error: null,
      changeError: null,
      application: reply.applyErrors.length > 0
        ? { kind: 'partial', errors: reply.applyErrors }
        : { kind: 'applied' },
    });
  } catch (e) {
    // A rejected `set_locale` is `Error::Prefs`: the choice was never
    // persisted. But persist-vs-transport is not recoverable from the
    // message alone, so this never guesses which — it marks the apply
    // outcome unknown and re-reads the confirmed choice instead. The message
    // itself is kept in `changeError`, never in `error`: `loadLocaleChoice`
    // below only ever touches `error`, so this one survives the recovery
    // read whichever way that read goes.
    state.update((s) => ({ ...s, busy: false, changeError: errorMessage(e), application: { kind: 'unknown' } }));
    await loadLocaleChoice();
  }
}

// Task 2's own retry path: repeating the CURRENT choice re-runs every apply
// step and the emit, so retrying is `changeLocaleChoice` with nothing new to
// decide. Needs a CONFIRMED snapshot to retry with — with none (the first
// read itself failed), there is no choice here to repeat; the failed-read
// recovery is `loadLocaleChoice()` again, a separate control in the UI.
// Review, PR44 P2-2: `s.snapshot === null` alone let a STALE, pre-write
// snapshot through — left behind by a rejected change whose own recovery
// read also failed — and retrying with it silently repeated the OLD choice,
// rolling back a write the backend had already persisted. `snapshotConfirmed`
// is what actually answers "is this the last thing a read or a command
// reply confirmed", which `snapshot !== null` never did on its own.
export async function retryLocaleApplication(): Promise<void> {
  const s = get(state);
  if (s.busy || s.snapshot === null || !s.snapshotConfirmed) return;
  await changeLocaleChoice(s.snapshot.choice);
}

// Test-only. The store above is module-level by design (the whole point is
// surviving a component remount), which means it also survives from one test
// to the next in the same file unless something resets it — `./theme`'s store
// resets with a bare `.set()` because it holds one field; this one is
// `Readable` on purpose, so a reset needs its own way in.
export function resetLocaleChoiceForTests(): void {
  state.set({ ...INITIAL });
  opSeq = 0;
}
