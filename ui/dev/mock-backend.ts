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
import { generated, historyTwoQuestions, oneRootTwoFolders } from '../src/lib/fixtures';
import type {
  AppPrefs, AskAnswer, Catalogue, RoleCatalogue, DocumentView, HistoryEntry, IndexRead, KeyState, LocaleChoice,
  ModelRole, ScanState, ThemeChoice, TreeListing, UiScale,
} from '../src/lib/ipc';

// The history as `list_history` keeps it, with the answer each question got.
type StoredQuestion = HistoryEntry & { answer: AskAnswer };

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
  const tree: TreeListing = fresh ? { roots: [] } : structuredClone(standTree);
  const history: StoredQuestion[] = fresh ? [] : historyTwoQuestions.map((h) => ({ ...h, answer: generated }));
  const scan: ScanState = {
    revision: 1, files: index.indexedFiles, readSeq: 1, jobsDone: 0, lastReading: null,
    snapshot: { kind: 'idle' },
  };
  const prefs: AppPrefs = {
    hotkey: { shortcut: 'Alt+Space', status: { kind: 'registered' } },
    autostart: { kind: 'disabled' },
    coldAfterMinutes: 5,
    uiScale: 'medium',
    version: '0.0.0-mock',
    platform: 'mac',
  };
  return {
    key: (fresh ? { kind: 'absent' } : { kind: 'present' }) as KeyState,
    index,
    tree,
    history,
    nextHistoryId: history.length + 1,
    masks: fresh ? [] : ['*.tmp'],
    exclusions: new Map<number, string[]>(),
    scan,
    prefs,
    theme: 'system' as ThemeChoice,
    locale: 'auto' as LocaleChoice,
    nextRootId: 100,
    chatTools: true,
  };
}

// Shaped like the real OpenRouter list, not a tidy one: the live catalogue
// always carries priced models, refused ones and a router, and the window
// draws the ⓘ and its notes only when it does (`Models.svelte`'s `hasNotes`).
// A clean list here hid that button once already.
const catalogue = (role: ModelRole, openRouter: boolean, mnema: boolean): RoleCatalogue => {
  const usd = (perMillion: number) => ({ kind: 'known' as const, amount: perMillion / 1e6 });
  const ok = (id: string, price: number, out = 0): Catalogue['entries'][number] => ({
    id, name: id.split('/')[1], inputLimit: { kind: 'known', tokens: 8192 },
    price: usd(price), outputPrice: usd(out), refusal: null, tools: false,
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
  // Mnema's own entry comes first and states no price; a provider that is not set up lists nothing.
  const local = role === 'embedding'
    ? { ...ok('baai/bge-m3', 0), name: 'BAAI: bge-m3', price: { kind: 'notStated' as const }, outputPrice: { kind: 'notStated' as const } }
    : { ...ok('gemma-4-e2b-it', 0), name: 'Google: Gemma 4 E2B', price: { kind: 'notStated' as const }, outputPrice: { kind: 'notStated' as const } };
  return {
    entries: [
      ...(mnema ? [{ ...local, provider: 'mnema' as const }] : []),
      ...(openRouter ? entries.map((e) => ({ ...e, provider: 'openRouter' as const })) : []),
    ],
    unreadable: 0, unreadableRecords: [],
  };
};

// The stand's tree: the fixture's rows (the answers cite them) plus enough
// depth to show nesting, root-level files and the search checks.
const standTree: TreeListing = {
  roots: [
    {
      ...oneRootTwoFolders.roots[0],
      files: [
        ...oneRootTwoFolders.roots[0].files,
        { relativePath: 'notes/drafts/idea.md', documentId: 'doc-4' },
        { relativePath: 'notes/drafts/deep/plan.md', documentId: 'doc-5' },
        { relativePath: 'notes/drafts/deep/quarterly-planning-meetings/2026/minutes-of-the-october-review.md', documentId: 'doc-10' },
        { relativePath: 'README.md', documentId: 'doc-6' },
        { relativePath: 'package.json', documentId: 'doc-7' },
      ],
    },
    {
      rootId: 2, absolutePath: '/home/u/papers', searched: [], name: 'papers',
      files: [
        { relativePath: '2024/survey.pdf', documentId: 'doc-8' },
        { relativePath: 'index.md', documentId: 'doc-9' },
      ],
    },
  ],
};

// What the stand's files say, by documentId, for `find_in_files` to look in.
const standTexts: Record<string, string> = {
  'doc-1': 'Notes on the lease: the rent is due on the first of each month.',
  'doc-2': 'The second set of notes covers the deposit and the inspection.',
  'doc-3': 'An archived draft of the old lease, kept for reference.',
  'doc-4': 'An idea for a quarterly planning review.',
  'doc-5': 'The plan for the next quarter, with milestones.',
  'doc-10': 'Minutes of the October review: planning meetings move to Mondays.',
  'doc-6': 'Mnema reads a folder of documents and answers with cited sources.',
  'doc-9': 'An index of the papers in this folder.',
};

export function installMockBackend(win: 'settings' | 'launcher') {
  const query = new URLSearchParams(location.search);
  const scenario = query.get('scenario') ?? 'configured';
  // ?provider=ok|unreachable|missing picks what the launcher's cloud shows.
  const provider = query.get('provider') ?? 'ok';
  const s = initialState(scenario);
  const bump = () => { s.scan = { ...s.scan, revision: s.scan.revision + 1 }; };
  // Each role has its own provider; a fresh install has none (the settings Provider picker only chooses a card).
  const roles: { embed: 'openRouter' | 'mnema' | null; chat: 'openRouter' | 'mnema' | null } =
    scenario === 'fresh' ? { embed: null, chat: null } : { embed: 'openRouter', chat: 'openRouter' };
  const settings = () => ({
    key: s.key, index: s.index, platform: 'mac' as const,
    roles,
    configured: { openRouter: s.key.kind === 'present', mnemaEmbed: local.embed?.kind === 'ready', mnemaChat: local.chat?.kind === 'ready' },
  });

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

  // Mnema (local): ?local=nospace makes every download answer "no space".
  const local: Record<string, { kind: string; done?: number; total?: number }> = {
    embed: { kind: 'absent' }, chat: { kind: 'absent' },
  };
  const cancelled = new Set<string>();
  const downloadMock = (id: string) => new Promise<void>((resolve, reject) => {
    if (query.get('local') === 'nospace') { reject({ kind: 'noSpace', needed: 3.4e9, free: 1.2e9 }); return; }
    cancelled.delete(id);
    let done = 0;
    const tick = () => {
      if (cancelled.has(id)) { local[id] = { kind: 'absent' }; reject({ kind: 'cancelled' }); return; }
      done += 10;
      void emit('local-model-progress', { id, done, total: 100 });
      if (done >= 100) { local[id] = { kind: 'ready' }; resolve(); } else setTimeout(tick, 400);
    };
    local[id] = { kind: 'downloading', done: 0, total: 100 };
    setTimeout(tick, 400);
  });

  const handlers: Record<string, (a: Args) => unknown> = {
    mnema_available: () => true,
    local_models: () => Object.entries(local).map(([id, state]) => ({ id, state })),
    download_model: (a) => downloadMock(a.id as string),
    cancel_download: (a) => { cancelled.add(a.id as string); },
    remove_model: (a) => { local[a.id as string] = { kind: 'absent' }; },
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
    provider_models: (a) => catalogue(
      a.role as ModelRole, s.key.kind === 'present',
      local[a.role === 'embedding' ? 'embed' : 'chat']?.kind === 'ready',
    ),
    set_chat_model: (a) => { roles.chat = a.provider as 'openRouter' | 'mnema'; s.index = { ...s.index, chatModel: a.model as string }; },
    set_embedding_model: (a) => {
      roles.embed = a.provider as 'openRouter' | 'mnema';
      s.index = { ...s.index, embeddingModel: a.model as string, embeddedChunks: 0, embeddedChunksEverywhere: 0 };
      return { model: a.model, dim: 1536, spaceId: 2, created: true, retired: [], index: s.index };
    },

    list_tree: () => s.tree,
    // The core's semantics: drop the root's entries under the folder, then set its own.
    set_folder_searched: (a) => {
      const f = a.folderPath as string;
      s.tree = {
        ...s.tree,
        roots: s.tree.roots.map((r) => r.rootId !== a.rootId ? r : {
          ...r,
          searched: [
            ...r.searched.filter((e) => f === '' ? false : !e.folderPath.startsWith(`${f}/`) && e.folderPath !== f),
            { folderPath: f, searched: a.searched as boolean },
          ],
        }),
      };
    },
    add_watched_folder: (a) => {
      const rootId = s.nextRootId++;
      const path = a.path as string;
      s.tree = { ...s.tree, roots: [...s.tree.roots, { rootId, absolutePath: path, name: path.split('/').pop()!, files: [], searched: [] }] };
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
    set_cold_after: (a) => {
      if (a.minutes === 0) throw 'the launcher needs at least one minute out of focus before the local model is unloaded';
      s.prefs = { ...s.prefs, coldAfterMinutes: a.minutes as number };
      return s.prefs.coldAfterMinutes;
    },
    // Stored only: the real command zooms every window, which a browser tab
    // cannot do.
    // The command falls back to medium for anything unknown; the stand does the
    // same, or it would show a fourth state the application never persists.
    set_ui_scale: (a) => {
      const choice = a.choice as UiScale;
      s.prefs = { ...s.prefs, uiScale: ['small', 'medium', 'large'].includes(choice) ? choice : 'medium' };
    },
    chat_tools_enabled: () => s.chatTools,
    new_chat: () => {},
    set_chat_tools: (a) => ({ kind: 'stored', on: (s.chatTools = a.on as boolean) }),
    provider_status: () => {
      const one = provider === 'unreachable' ? { kind: 'unreachable', reason: 'connection timed out' }
        : provider === 'missing' ? { kind: 'notConfigured', missing: 'key' }
        : { kind: 'ok' };
      return { embed: one, chat: one };
    },
    open_settings: (a) => {
      win === 'launcher' && open('/dev/settings.html' + location.search, '_blank');
      // The bench's event bus is page-local, so the section reaches only this
      // page; the settings tab opened above starts on its default section.
      if (a.section) void emit('settings-section', a.section);
    },

    set_search_arms: (a) => { s.index = { ...s.index, searchTextArm: !!a.text, searchContentArm: !!a.content }; },
    ask: (a) => {
      // The same rule the backend keeps: one row per trimmed query, moved to the top.
      const query = (a.query as string).trim();
      // The backend cuts the answer's opening to one line for the sidebar; the
      // bench keeps the demo answer's own prose, its citation markers stripped.
      const preview = (generated.kind === 'generated' ? generated.answer : '').replace(/<c>\d+<\/c>/g, '');
      s.history = [
        { id: s.nextHistoryId++, query, preview, askedAt: Math.floor(Date.now() / 1000), answer: generated },
        ...s.history.filter((h) => h.query !== query),
      ];
      // `AskReply`, not the bare answer: the launcher reads `reply.answer` and
      // marks `reply.historyId`, and `history_entry` below keeps the bare answer
      // because that is what a `StoredTurn` holds.
      return { answer: generated, historyId: s.history[0].id };
    },
    // The backend matches the question and the WHOLE stored answer, caselessly;
    // the bench does the same over its stored `answer`, not the drawn preview.
    list_history: (a) => {
      const words = ((a.filter as string | undefined) ?? '').trim().toLowerCase();
      const answerOf = (h: StoredQuestion) => (h.answer.kind === 'generated' ? h.answer.answer : '');
      return s.history
        .filter((h) => !words || `${h.query}\n${answerOf(h)}`.toLowerCase().includes(words))
        .map(({ id, query, preview, askedAt }) => ({ id, query, preview, askedAt }));
    },
    // Substring, caseless, one best chunk per file; the backend matches word
    // prefixes in the index, the bench only has to be close enough to draw.
    find_in_files: (a) => {
      const q = ((a.query as string) ?? '').trim().toLowerCase();
      if (!/[\p{L}\p{N}]/u.test(q)) return null;
      const documents = Object.entries(standTexts)
        .filter(([, text]) => text.toLowerCase().includes(q))
        .map(([documentId, text], i) => ({ documentId, chunkId: i + 1, ord: 0, text }));
      return { documents, truncated: false };
    },
    history_entry: (a) => {
      const hit = s.history.find((h) => h.id === a.id);
      return hit ? { kind: 'dialogue', turns: [{ query: hit.query, answer: hit.answer }] } : { kind: 'gone' };
    },
    continue_history: (a) => s.history.some((h) => h.id === a.id),
    delete_history_entry: (a) => { s.history = s.history.filter((h) => h.id !== a.id); },
    // The two shapes beside the blocks are what a citation click reads: the
    // block an answer points at, and a span inside it. Empty ones made every
    // click say "the passage is gone" instead of lighting the passage up, so
    // this points at the demo paragraph the way a real citation would.
    open_document: () => {
      const paragraph = 'The whole file, rendered by the pane.';
      return {
        kind: 'file',
        blocks: [
          { blockId: 1, kind: 'headline', text: '## Demo document', pageNo: 1, sectionTitle: null },
          { blockId: 2, kind: 'paragraph', text: paragraph, pageNo: 1, sectionTitle: null },
        ],
        focusBlockIds: [2],
        spans: [{ blockId: 2, blockStart: 0, length: paragraph.length, primary: true }],
        relativePath: 'notes/demo.md',
        freshness: { kind: 'current' },
      } satisfies DocumentView;
    },

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
