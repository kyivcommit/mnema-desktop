import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import { unmount } from 'svelte';

// No test imported either `main.ts` (§15.5): a deleted `bootTheme()` or
// `bootLocale()` left every suite green and one window on its default.
// This file mounts THROUGH the entry points and counts the two boot calls.
//
// Partial mocks: only the two boot functions are spies. The stores behind
// them (`theme`, `locale`, `t`) are the real ones — `Settings.svelte` reads
// `t` at mount, and a whole-module mock would break it before the count.
const h = vi.hoisted(() => ({
  bootTheme: vi.fn(async () => {}),
  bootLocale: vi.fn(async () => {}),
}));
vi.mock('./theme', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./theme')>()),
  bootTheme: h.bootTheme,
}));
vi.mock('./i18n', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./i18n')>()),
  bootLocale: h.bootLocale,
}));
// IPC that never answers: every wrapper returns a promise that never settles,
// so both windows mount in their "reading" state and take no further step.
// The non-function exports (`END_REASONS` and the other tables) stay real.
vi.mock('./lib/ipc', async (importOriginal) => {
  const real = await importOriginal<Record<string, unknown>>();
  const never = () => new Promise<never>(() => {});
  return Object.fromEntries(
    Object.entries(real).map(([name, value]) => [name, typeof value === 'function' ? never : value]),
  );
});
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => ({ hide: vi.fn() }) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

// The entry points `mount()` on import and hand the instance back as their
// default export; Testing Library never sees it, so teardown is ours.
// `Settings.svelte`'s `onMount(() => jobs.mount())` opens the scan-progress
// subscription, `Scanning.svelte`'s `$effect` starts a `setInterval`,
// `JobStrip.svelte`'s `onMount` adds `focusin`/`pointerdown` listeners on
// `document` — all of which would outlive the test without `unmount`.
let app: Record<string, unknown> | null = null;

beforeEach(() => {
  // `clearMocks` is off in `vite.config.ts`; without this the second test
  // would see the first test's call on each spy.
  vi.clearAllMocks();
  document.body.innerHTML = '<div id="app"></div>';
});

afterEach(async () => {
  // `unmount` returns a Promise in this Svelte (types/index.d.ts:581); an
  // un-awaited one lets the next test start before the listeners are gone.
  if (app !== null) await unmount(app);
  app = null;
  document.body.innerHTML = '';
});

test('the launcher entry point boots the locale and the theme once each', async () => {
  app = (await import('./launcher/main')).default as Record<string, unknown>;
  expect(h.bootLocale).toHaveBeenCalledTimes(1);
  expect(h.bootTheme).toHaveBeenCalledTimes(1);
  expect(document.getElementById('app')!.childElementCount).toBeGreaterThan(0);
});

test('the settings entry point boots the locale and the theme once each', async () => {
  app = (await import('./settings/main')).default as Record<string, unknown>;
  expect(h.bootLocale).toHaveBeenCalledTimes(1);
  expect(h.bootTheme).toHaveBeenCalledTimes(1);
  expect(document.getElementById('app')!.childElementCount).toBeGreaterThan(0);
});
