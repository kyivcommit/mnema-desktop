import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { locale, setLocale } from './i18n';

// `locale-choice.ts` owns the language selection Task 3 moves into the
// Application section. It is a MODULE-level store (like `./theme`), not a
// component field: `Settings.svelte` destroys and recreates `Application`
// on every section switch (`{#if section === 'application'}`), so anything
// that must survive that remount — busy, a partial-apply warning, the
// confirmed snapshot — has to live somewhere that remount does not touch.
const getLocale = vi.fn();
const setLocaleChoice = vi.fn();
vi.mock('./lib/ipc', () => ({
  getLocale: (...a: unknown[]) => getLocale(...a),
  setLocaleChoice: (...a: unknown[]) => setLocaleChoice(...a),
}));

import {
  localeChoiceState,
  loadLocaleChoice,
  changeLocaleChoice,
  retryLocaleApplication,
  resetLocaleChoiceForTests,
} from './locale-choice';

beforeEach(() => {
  getLocale.mockReset();
  setLocaleChoice.mockReset();
  resetLocaleChoiceForTests();
  setLocale('en');
});

describe('loadLocaleChoice', () => {
  it('confirms choice/effective from a successful read', async () => {
    getLocale.mockResolvedValue({ choice: 'auto', effective: 'en' });
    await loadLocaleChoice();
    expect(get(localeChoiceState).snapshot).toEqual({ choice: 'auto', effective: 'en' });
    expect(get(localeChoiceState).error).toBeNull();
  });

  // "невідомий command result + failed read": a rejected change leaves
  // `application` at `unknown` and error set; this is the OTHER half — a read
  // failing on its own must leave the same two things, not silently keep
  // whatever the store held before.
  it('a failed read leaves error set and application unknown', async () => {
    getLocale.mockRejectedValue(new Error('disk unreadable'));
    await loadLocaleChoice();
    const s = get(localeChoiceState);
    expect(s.error).toBe('disk unreadable');
    expect(s.application).toEqual({ kind: 'unknown' });
  });

  // "збереження warning при load": a plain read must never clear a partial
  // warning a previous change earned — it has no `applyErrors` to confirm one
  // way or the other, so it is not entitled to an opinion about it.
  it('does not clear a partial warning', async () => {
    setLocaleChoice.mockResolvedValue({
      choice: 'en', effective: 'en',
      applyErrors: [{ surface: 'tray', message: 'tray refused' }],
    });
    await changeLocaleChoice('en');
    expect(get(localeChoiceState).application.kind).toBe('partial');

    getLocale.mockResolvedValue({ choice: 'en', effective: 'en' });
    await loadLocaleChoice();
    const s = get(localeChoiceState);
    expect(s.application).toEqual({
      kind: 'partial', errors: [{ surface: 'tray', message: 'tray refused' }],
    });
    expect(s.snapshot).toEqual({ choice: 'en', effective: 'en' });
  });

  // "remount-equivalent повторний load під час busy": `Application` mounting
  // again while an older change is still in flight (remount landed mid-write)
  // must not start a second, concurrent read — it will see the change's own
  // result once that settles.
  it('a load started while busy does not start a concurrent read', async () => {
    let finishChange!: (v: { choice: string; effective: 'en'; applyErrors: [] }) => void;
    setLocaleChoice.mockImplementationOnce(() => new Promise((res) => { finishChange = res as never; }));
    const change = changeLocaleChoice('en');
    expect(get(localeChoiceState).busy).toBe(true);

    await loadLocaleChoice(); // the remount's own mount-time load
    expect(getLocale).not.toHaveBeenCalled();

    finishChange({ choice: 'en', effective: 'en', applyErrors: [] });
    await change;
    expect(get(localeChoiceState).busy).toBe(false);
  });

  // "старий load після change": a read in flight BEFORE a change starts must
  // not overwrite what the change confirmed once it finally resolves — the
  // change is the newer operation and the read predates it.
  it('a stale read that resolves after a newer change does not repaint the old choice', async () => {
    let finishRead!: (v: { choice: string; effective: 'en' }) => void;
    getLocale.mockImplementationOnce(() => new Promise((res) => { finishRead = res as never; }));
    const staleLoad = loadLocaleChoice();

    setLocaleChoice.mockResolvedValueOnce({ choice: 'uk', effective: 'uk', applyErrors: [] });
    await changeLocaleChoice('uk');
    expect(get(localeChoiceState).snapshot).toEqual({ choice: 'uk', effective: 'uk' });

    finishRead({ choice: 'auto', effective: 'en' }); // the stale reply arrives late
    await staleLoad;
    expect(get(localeChoiceState).snapshot).toEqual({ choice: 'uk', effective: 'uk' }); // unchanged
  });
});

describe('changeLocaleChoice', () => {
  it('an applied reply (no applyErrors) sets application to applied and moves the i18n locale', async () => {
    setLocaleChoice.mockResolvedValue({ choice: 'uk', effective: 'uk', applyErrors: [] });
    await changeLocaleChoice('uk');
    const s = get(localeChoiceState);
    expect(s.application).toEqual({ kind: 'applied' });
    expect(s.snapshot).toEqual({ choice: 'uk', effective: 'uk' });
    expect(s.busy).toBe(false);
    expect(s.error).toBeNull();
    expect(get(locale)).toBe('uk'); // applied through the confirmed reply, not guessed from the request
  });

  // "partial reply з En": a reply carrying applyErrors is not a rejection —
  // choice/effective are confirmed, and the warning names which surfaces did
  // not pick it up.
  it('a partial reply keeps the confirmed choice and records which surfaces failed', async () => {
    setLocaleChoice.mockResolvedValue({
      choice: 'en', effective: 'en',
      applyErrors: [{ surface: 'appMenu', message: 'menu not open' }],
    });
    await changeLocaleChoice('en');
    const s = get(localeChoiceState);
    expect(s.application).toEqual({
      kind: 'partial', errors: [{ surface: 'appMenu', message: 'menu not open' }],
    });
    expect(s.snapshot).toEqual({ choice: 'en', effective: 'en' });
    expect(get(locale)).toBe('en');
  });

  // "невідомий command result": a rejected `set_locale` does not mean the
  // choice failed to persist — persist-vs-transport is not recoverable from
  // the message — so it re-reads instead of guessing, and marks `application`
  // unknown rather than keeping a stale `applied`/`partial`.
  it('a rejected change re-reads instead of guessing, and marks application unknown', async () => {
    setLocaleChoice.mockRejectedValue(new Error('IPC closed'));
    getLocale.mockResolvedValue({ choice: 'uk', effective: 'uk' });
    await changeLocaleChoice('uk');
    const s = get(localeChoiceState);
    expect(s.application).toEqual({ kind: 'unknown' });
    expect(s.snapshot).toEqual({ choice: 'uk', effective: 'uk' }); // from the re-read, not guessed
    expect(s.busy).toBe(false);
    expect(getLocale).toHaveBeenCalledTimes(1);
  });

  // "невідомий command result + failed read": the recovery read can itself
  // fail; the error left behind is the READ's, and application stays unknown.
  it('a rejected change whose recovery read also fails leaves an error and unknown', async () => {
    setLocaleChoice.mockRejectedValue(new Error('IPC closed'));
    getLocale.mockRejectedValue(new Error('disk unreadable'));
    await changeLocaleChoice('uk');
    const s = get(localeChoiceState);
    expect(s.application).toEqual({ kind: 'unknown' });
    expect(s.error).toBe('disk unreadable');
    expect(s.busy).toBe(false);
  });

  it('a change started while busy does not start a second command', async () => {
    let finish!: (v: { choice: string; effective: 'uk'; applyErrors: [] }) => void;
    setLocaleChoice.mockImplementationOnce(() => new Promise((res) => { finish = res as never; }));
    const first = changeLocaleChoice('uk');
    await changeLocaleChoice('en'); // a second choice pressed while the first is in flight
    expect(setLocaleChoice).toHaveBeenCalledTimes(1);
    expect(setLocaleChoice).toHaveBeenCalledWith('uk');
    finish({ choice: 'uk', effective: 'uk', applyErrors: [] });
    await first;
  });
});

describe('retryLocaleApplication', () => {
  // "retry того самого choice": Task 2 made repeating the same choice re-run
  // every apply step and the emit — that IS the retry path, so retry is
  // `changeLocaleChoice(snapshot.choice)` and nothing more.
  it('retries with the current snapshot\'s choice', async () => {
    setLocaleChoice.mockResolvedValueOnce({
      choice: 'en', effective: 'en',
      applyErrors: [{ surface: 'tray', message: 'tray refused' }],
    });
    await changeLocaleChoice('en');
    expect(get(localeChoiceState).application.kind).toBe('partial');

    setLocaleChoice.mockResolvedValueOnce({ choice: 'en', effective: 'en', applyErrors: [] });
    await retryLocaleApplication();
    expect(setLocaleChoice).toHaveBeenLastCalledWith('en');
    // "повний retry success": the warning clears only once the retry itself
    // comes back clean, not just because it was pressed.
    expect(get(localeChoiceState).application).toEqual({ kind: 'applied' });
  });

  it('does nothing without a known current snapshot', async () => {
    await retryLocaleApplication(); // no prior load/change: snapshot is still null
    expect(setLocaleChoice).not.toHaveBeenCalled();
  });

  it('does nothing while busy', async () => {
    let finish!: (v: { choice: string; effective: 'uk'; applyErrors: [] }) => void;
    setLocaleChoice.mockImplementationOnce(() => new Promise((res) => { finish = res as never; }));
    const first = changeLocaleChoice('uk');
    await retryLocaleApplication();
    expect(setLocaleChoice).toHaveBeenCalledTimes(1);
    finish({ choice: 'uk', effective: 'uk', applyErrors: [] });
    await first;
  });
});
