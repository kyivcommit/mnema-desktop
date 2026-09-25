// Dev-only stand-in for the Rust side, so the two windows run in a plain
// browser tab (Vite dev server, `npm run dev`, then /dev/settings.html or
// /dev/launcher.html). Nothing here is built: `vite.config.ts` lists only the
// two real entry points in `rollupOptions.input`.
//
// It answers every command `src/lib/ipc.ts`, `theme.ts` and `i18n/index.ts`
// call, with plausible state, and keeps that state across calls so a press
// shows its effect. It is a layout bench, not a contract: the wire types are
// imported from `ipc.ts`, so a renamed field fails `npm run check` here too,
// but the behaviour is a sketch of the backend's, never a copy of it.
//
// Scenarios: `?scenario=configured` (default) or `?scenario=fresh` (no key, no
// folders, never indexed). From the browser console, `mnemaMock.state` is the
// live state and `mnemaMock.emit(event, payload)` fires a backend event.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { excerptSpanA, generated, oneRootTwoFolders } from '../src/lib/fixtures';
import type {
  AppPrefs, Catalogue, IndexRead, KeyState, LocaleChoice, ModelRole, ScanState, ThemeChoice,
  TreeListing,
} from '../src/lib/ipc';

type Args = Record<string, unknown>;

function initialState(scenario: string) {
  const fresh = scenario === 'fresh';
  const index: IndexRead = {
    kind: 'read',
    embeddingModel: fresh ? null : 'openai/text-embedding-3-small',
    chatModel: fresh ? null : 'meta-llama/llama-3.3-70b-instruct:free',
    embeddedChunks: fresh ? 0 : 1840,
    embeddedChunksEverywhere: fresh ? 0 : 1840,
    totalChunks: fresh ? 0 : 1840,
    failedChunks: 0,
    pendingChunks: 0,
    indexedFiles: fresh ? 0 : 3,
    lastIndexedAt: fresh ? null : Math.floor(Date.now() / 1000) - 3600,
    scanIncomplete: false,
    searchTextArm: true,
    searchContentArm: !fresh,
  };
  const tree: TreeListing = fresh ? { roots: [], recents: [] } : structuredClone(oneRootTwoFolders);
  const scan: ScanState = {
    revision: 1, files: index.indexedFiles, readSeq: 1, jobsDone: 0, lastReading: null,
    snapshot: { kind: 'idle' },
  };
  const prefs: AppPrefs = {
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'disabled' },
    version: '0.0.0-mock',
    platform: 'mac',
  };
  return {
    key: (fresh ? { kind: 'absent' } : { kind: 'present' }) as KeyState,
    index,
    tree,
    masks: fresh ? [] : ['*.tmp'],
    exclusions: new Map<number, string[]>(),
    scan,
    prefs,
    theme: 'system' as ThemeChoice,
    locale: 'auto' as LocaleChoice,
    nextRootId: 100,
  };
}

// Shaped like the real OpenRouter list, not a tidy one: the live catalogue
// always carries priced models, refused ones and a router, and the window
// draws the ⓘ and its notes only when it does (`Models.svelte`'s `hasNotes`).
// A clean list here hid that button once already.
const catalogue = (role: ModelRole): Catalogue => {
  const usd = (perMillion: number) => ({ kind: 'known' as const, amount: perMillion / 1e6 });
  const ok = (id: string, price: number, out = 0): Catalogue['entries'][number] => ({
    id, name: id.split('/')[1], inputLimit: { kind: 'known', tokens: 8192 },
    price: usd(price), outputPrice: usd(out), refusal: null,
  });
  const refused = (id: string, refusal: NonNullable<Catalogue['entries'][number]['refusal']>) =>
    ({ ...ok(id, 0), refusal });
  const entries = role === 'embedding'
    ? [
        ok('openai/text-embedding-3-small', 0.02),
        ok('openai/text-embedding-3-large', 0.13),
        ok('qwen/qwen3-embedding-8b', 0.01),
        refused('thenlper/gte-base', { kind: 'inputTooSmall', limit: 512, floor: 2048 }),
        refused('baai/bge-m3', { kind: 'noStatedLimit' }),
      ]
    : [
        ok('meta-llama/llama-3.3-70b-instruct:free', 0),
        ok('google/gemma-3-27b-it:free', 0),
        ok('anthropic/claude-sonnet-4.5', 3, 15),
        refused('openrouter/auto', { kind: 'router' }),
        refused('openai/dall-e-3', { kind: 'noTextOutput' }),
      ];
  return { entries, unreadable: 0, unreadableRecords: [] };
};

export function installMockBackend(win: 'settings' | 'launcher') {
  const scenario = new URLSearchParams(location.search).get('scenario') ?? 'configured';
  const s = initialState(scenario);
  const bump = () => { s.scan = { ...s.scan, revision: s.scan.revision + 1 }; };
  const settings = () => ({ key: s.key, index: s.index, platform: 'mac' as const });

  // ponytail: a scan is three timer ticks, not the real phase machine; enough
  // to draw the progress strip, add phases here when a task needs them.
  function runScan() {
    const total = Math.max(s.tree.roots.reduce((n, r) => n + r.files.length, 0), 1);
    const root = s.tree.roots[0]?.absolutePath ?? '/';
    let done = 0;
    const tick = () => {
      if (s.scan.snapshot.kind !== 'running') return;
      done += 1;
      if (done > total) {
        s.scan = { ...s.scan, readSeq: s.scan.readSeq + 1, jobsDone: s.scan.jobsDone + 1, snapshot: {
          kind: 'ended',
          report: { embedding: { kind: 'ran', done: total, total, refused: 0 }, endedIn: 'embedding',
            reason: 'completed', message: null, resume: null },
        } };
        s.index = { ...s.index, lastIndexedAt: Math.floor(Date.now() / 1000) };
      } else {
        s.scan = { ...s.scan, snapshot: { kind: 'running', cancellable: true, phase: {
          kind: 'reading', rootIndex: 0, rootCount: s.tree.roots.length, rootPath: root,
          counts: { done, total, skipped: 0, refused: 0, contended: 0, secondsLeft: total - done },
        } } };
        setTimeout(tick, 1000);
      }
      bump();
      void emit('scan-progress', s.scan);
    };
    s.scan = { ...s.scan, snapshot: { kind: 'running', cancellable: true, phase: { kind: 'embedding',
      counts: { done: 0, total, skipped: 0, refused: 0, contended: 0, secondsLeft: null } } } };
    setTimeout(tick, 300);
  }

  const handlers: Record<string, (a: Args) => unknown> = {
    get_theme: () => ({ choice: s.theme }),
    set_theme: (a) => { s.theme = a.choice as ThemeChoice; void emit('theme-changed', s.theme); },
    get_locale: () => ({ choice: s.locale, effective: s.locale === 'uk' ? 'uk' : 'en' }),
    set_locale: (a) => {
      s.locale = a.choice as LocaleChoice;
      const effective = s.locale === 'uk' ? 'uk' : 'en';
      void emit('locale-changed', effective);
      return { choice: s.locale, effective, applyErrors: [] };
    },

    model_settings: settings,
    set_key: () => { s.key = { kind: 'present' }; return { balance: { kind: 'known', amount: 4.2 } }; },
    forget_key: () => {
      const had = s.key.kind === 'present';
      s.key = { kind: 'absent' };
      return { kind: had ? 'removed' : 'nothingToRemove' };
    },
    provider_models: (a) => catalogue(a.role as ModelRole),
    set_chat_model: (a) => { s.index = { ...s.index, chatModel: a.model as string }; },
    set_embedding_model: (a) => {
      s.index = { ...s.index, embeddingModel: a.model as string, embeddedChunks: 0, embeddedChunksEverywhere: 0 };
      return { model: a.model, dim: 1536, spaceId: 2, created: true, retired: [], index: s.index };
    },

    list_tree: () => s.tree,
    add_watched_folder: (a) => {
      const rootId = s.nextRootId++;
      const path = a.path as string;
      s.tree = { ...s.tree, roots: [...s.tree.roots, { rootId, absolutePath: path, name: path.split('/').pop()!, files: [] }] };
      return rootId;
    },
    remove_watched_folder: (a) => {
      const before = s.tree.roots.length;
      s.tree = { ...s.tree, roots: s.tree.roots.filter((r) => r.rootId !== a.rootId) };
      return before - s.tree.roots.length;
    },
    list_subfolders: (a) => ({
      entries: a.relativePath === ''
        ? [
            { name: 'archive', relativePath: 'archive', state: { kind: 'open' } },
            { name: 'drafts', relativePath: 'drafts',
              state: (s.exclusions.get(a.rootId as number) ?? []).includes('drafts') ? { kind: 'excluded' } : { kind: 'open' } },
            { name: 'node_modules', relativePath: 'node_modules', state: { kind: 'builtIn' } },
          ]
        : [],
      unnameable: 0,
    }),
    list_exclusions: (a) => (s.exclusions.get(a.rootId as number) ?? []).map((prefix) => ({ prefix, existsOnDisk: true })),
    exclude_subfolder: (a) => {
      const id = a.rootId as number;
      s.exclusions.set(id, [...(s.exclusions.get(id) ?? []), a.relativePath as string]);
    },
    include_subfolder: (a) => {
      const id = a.rootId as number;
      const list = s.exclusions.get(id) ?? [];
      s.exclusions.set(id, list.filter((p) => p !== a.relativePath));
      return list.includes(a.relativePath as string);
    },

    list_masks: () => s.masks,
    mask_preview: () => ({ paths: 2, documents: 1 }),
    add_mask: (a) => {
      const p = a.pattern as string;
      const same = s.masks.find((m) => m.toLowerCase() === p.toLowerCase());
      if (same) return { kind: 'alreadyStored', stored: same };
      s.masks = [...s.masks, p];
      return { kind: 'stored' };
    },
    remove_mask: (a) => {
      const before = s.masks.length;
      s.masks = s.masks.filter((m) => m !== a.pattern);
      return before !== s.masks.length;
    },

    job_status: () => s.scan,
    start_scan_job: () => runScan(),
    cancel_job: () => {
      s.scan = { ...s.scan, snapshot: { kind: 'ended', report: { embedding: { kind: 'notReached' },
        endedIn: 'reading', reason: 'cancelled', message: null, resume: 'full' } } };
      bump();
      void emit('scan-progress', s.scan);
    },

    app_prefs: () => s.prefs,
    set_hotkey: (a) => {
      s.prefs = { ...s.prefs, hotkey: { shortcut: a.shortcut as string, status: { kind: 'registered' } } };
      return s.prefs.hotkey;
    },
    set_autostart: (a) => {
      s.prefs = { ...s.prefs, autostart: { kind: a.enabled ? 'enabled' : 'disabled' } };
      return s.prefs.autostart;
    },
    open_settings: () => { win === 'launcher' && open('/dev/settings.html' + location.search, '_blank'); },

    set_search_arms: (a) => { s.index = { ...s.index, searchTextArm: !!a.text, searchContentArm: !!a.content }; },
    ask: () => generated,
    source_around: () => excerptSpanA,

    'plugin:dialog|open': () => '/Users/demo/Documents/Projects',
  };

  mockWindows(win);
  mockIPC((cmd, payload) => {
    const handler = handlers[cmd];
    if (handler) return handler((payload ?? {}) as Args);
    if (cmd.startsWith('plugin:window|')) return null;
    // Loud on purpose: a command this bench does not know is a gap in it, and
    // the component's own error branch would otherwise make it look deliberate.
    console.warn(`[mock] no handler for ${cmd}`, payload);
    throw new Error(`mock backend: no handler for ${cmd}`);
  }, { shouldMockEvents: true });

  Object.assign(globalThis, { mnemaMock: { state: s, emit } });
}
