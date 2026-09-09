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
  busy: boolean;
  error: string | null;
  application: LocaleApplicationState;
};

const INITIAL: LocaleChoiceState = {
  snapshot: null, busy: false, error: null, application: { kind: 'initial' },
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
    state.update((s) => ({ ...s, snapshot: reply, error: null }));
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
  const mine = ++opSeq;
  state.update((s) => ({ ...s, busy: true }));
  try {
    const reply = await setLocaleChoice(choice);
    if (mine !== opSeq) return;
    // The confirmed reply is the truth, applied through the SAME setter
    // `bootLocale` uses — never guessed from `choice`, the request this
    // window sent, which is not necessarily what a persist step wrote.
    applyEffectiveLocale(reply.effective);
    state.set({
      snapshot: { choice: reply.choice, effective: reply.effective },
      busy: false,
      error: null,
      application: reply.applyErrors.length > 0
        ? { kind: 'partial', errors: reply.applyErrors }
        : { kind: 'applied' },
    });
  } catch (e) {
    if (mine !== opSeq) return;
    // A rejected `set_locale` is `Error::Prefs`: the choice was never
    // persisted. But persist-vs-transport is not recoverable from the
    // message alone, so this never guesses which — it marks the apply
    // outcome unknown and re-reads the confirmed choice instead.
    state.update((s) => ({ ...s, busy: false, error: errorMessage(e), application: { kind: 'unknown' } }));
    await loadLocaleChoice();
  }
}

// Task 2's own retry path: repeating the CURRENT choice re-runs every apply
// step and the emit, so retrying is `changeLocaleChoice` with nothing new to
// decide. Needs a confirmed snapshot to retry with — with none (the first
// read itself failed), there is no choice here to repeat; the failed-read
// recovery is `loadLocaleChoice()` again, a separate control in the UI.
export async function retryLocaleApplication(): Promise<void> {
  const s = get(state);
  if (s.busy || s.snapshot === null) return;
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
