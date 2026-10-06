import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Provider from './Provider.svelte';
import { setLocale } from '../i18n';
import type {
  LocalModelProgress, LocalModelRow, ModelSettings, ProviderChoice,
} from '../lib/ipc';

const mnemaAvailable = vi.fn();
const providerChoice = vi.fn();
const setProviderChoice = vi.fn();
const localModels = vi.fn();
const downloadModel = vi.fn();
const cancelDownload = vi.fn();
const removeModel = vi.fn();
const modelSettings = vi.fn();
const unlisten = vi.fn();
// The `local-model-progress` handler the mounted component registered.
let deliver: ((p: LocalModelProgress) => void) | null = null;
vi.mock('../lib/ipc', () => ({
  mnemaAvailable: (...a: unknown[]) => mnemaAvailable(...a),
  providerChoice: (...a: unknown[]) => providerChoice(...a),
  setProviderChoice: (...a: unknown[]) => setProviderChoice(...a),
  localModels: (...a: unknown[]) => localModels(...a),
  downloadModel: (...a: unknown[]) => downloadModel(...a),
  cancelDownload: (...a: unknown[]) => cancelDownload(...a),
  removeModel: (...a: unknown[]) => removeModel(...a),
  modelSettings: (...a: unknown[]) => modelSettings(...a),
  listenLocalModelProgress: (cb: (p: LocalModelProgress) => void) => {
    deliver = cb;
    return Promise.resolve(unlisten);
  },
}));

const ABSENT: LocalModelRow[] = [
  { id: 'embed', state: { kind: 'absent' } },
  { id: 'chat', state: { kind: 'absent' } },
];
const rows = (embed: LocalModelRow['state'], chat: LocalModelRow['state']): LocalModelRow[] => [
  { id: 'embed', state: embed }, { id: 'chat', state: chat },
];
const READY = { kind: 'ready' } as const;

function settings(embeddingModel: string | null, everywhere: number): ModelSettings {
  return {
    key: { kind: 'absent' },
    index: {
      kind: 'read', embeddingModel, embeddedChunks: everywhere, embeddedChunksEverywhere: everywhere,
      totalChunks: everywhere, failedChunks: 0, pendingChunks: 0, indexedFiles: 1,
      lastIndexedAt: null, scanIncomplete: false, searchTextArm: true, searchContentArm: true,
    },
    platform: 'mac',
  };
}

// The OpenRouter key field the section hosts, stood in for by a labelled input
// handed over as `children` — what `Settings.svelte` does with `<Models>`.
const keyField = createRawSnippet(() => ({
  render: () => '<label>Ключ: <input type="password" /></label>',
}));

// Lets every already-resolved mock answer reach the component.
const settle = () => new Promise<void>((r) => setTimeout(r, 0));

async function mount(over: {
  available?: boolean; choice?: ProviderChoice; rows?: LocalModelRow[]; settings?: ModelSettings;
} = {}) {
  mnemaAvailable.mockResolvedValue(over.available ?? true);
  providerChoice.mockResolvedValue(over.choice ?? 'openRouter');
  localModels.mockResolvedValue(over.rows ?? ABSENT);
  modelSettings.mockResolvedValue(over.settings ?? settings('baai/bge-m3', 0));
  const view = render(Provider, { children: keyField });
  await settle();
  return view;
}

const MNEMA = { name: 'Mnema (локально)' };

beforeEach(() => {
  for (const f of [mnemaAvailable, providerChoice, setProviderChoice, localModels, downloadModel,
    cancelDownload, removeModel, modelSettings, unlisten]) f.mockReset();
  deliver = null;
  setProviderChoice.mockImplementation(async (choice: ProviderChoice) => ({ choice, retired: [] }));
  downloadModel.mockResolvedValue(undefined);
  cancelDownload.mockResolvedValue(undefined);
  removeModel.mockResolvedValue(undefined);
  setLocale('uk');
});
afterEach(() => { cleanup(); setLocale('en'); });

test('hidden when mnema is unavailable', async () => {
  await mount({ available: false });
  expect(screen.queryByRole('radio', MNEMA)).toBeNull();
  cleanup();
  await mount({ available: true });
  expect(await screen.findByRole('radio', MNEMA)).toBeTruthy();
});

test('choosing mnema hides the key field', async () => {
  await mount();
  // Both directions: under OpenRouter the key field is there and the hint is not.
  expect(screen.getByLabelText('Ключ:')).toBeTruthy();
  expect(screen.queryByText("~3,1 ГБ диска, ~4 ГБ пам'яті")).toBeNull();
  await fireEvent.click(await screen.findByRole('radio', MNEMA));
  await waitFor(() => expect(screen.queryByLabelText('Ключ:')).toBeNull());
  expect(setProviderChoice).toHaveBeenCalledWith('mnema', 'keep');
  expect(screen.getByText("~3,1 ГБ диска, ~4 ГБ пам'яті")).toBeTruthy();
  // And back: the key field returns with the choice.
  await fireEvent.click(screen.getByRole('radio', { name: 'OpenRouter' }));
  await waitFor(() => expect(screen.getByLabelText('Ключ:')).toBeTruthy());
  expect(setProviderChoice).toHaveBeenLastCalledWith('openRouter', 'keep');
});

// A mounted component with Mnema already chosen: the rows are what is under test.
const mountRows = (r: LocalModelRow[]) => mount({ choice: 'mnema', rows: r });
// The chat model's row, found by its heading so a button belongs to a model.
const chatRow = () => screen.getByRole('group', { name: 'Модель відповідей' });
const inRow = (row: HTMLElement) => ({ button: (name: string) => within(row).getByRole('button', { name }) });

test('a row walks absent → downloading → ready', async () => {
  await mountRows(ABSENT);
  let pending!: (v?: unknown) => void;
  downloadModel.mockImplementation(() => new Promise((r) => { pending = r; }));
  const row = await waitFor(() => chatRow());
  await fireEvent.click(inRow(row).button('Завантажити'));
  expect(downloadModel).toHaveBeenCalledWith('chat');
  deliver!({ id: 'chat', done: 50, total: 100 });
  const bar = await waitFor(() => {
    const b = chatRow().querySelector('[role="progressbar"]');
    expect(b).not.toBeNull();
    return b!;
  });
  expect(bar.getAttribute('aria-valuenow')).toBe('50');
  // The other model's row shows no bar: progress belongs to the row it names.
  expect(screen.getByRole('group', { name: 'Модель пошуку' }).querySelector('[role="progressbar"]')).toBeNull();
  await fireEvent.click(inRow(chatRow()).button('Скасувати'));
  expect(cancelDownload).toHaveBeenCalledWith('chat');
  // The download ends and the re-read says Ready: a tick and a way to remove it.
  localModels.mockResolvedValue(rows({ kind: 'absent' }, READY));
  pending();
  await waitFor(() => expect(chatRow().textContent).toContain('✓'));
  expect(chatRow().querySelector('[role="progressbar"]')).toBeNull();
  await fireEvent.click(inRow(chatRow()).button('Видалити'));
  expect(removeModel).toHaveBeenCalledWith('chat');
});

test('a failed row offers retry', async () => {
  await mountRows(rows({ kind: 'absent' }, { kind: 'failed', message: 'checksum mismatch' }));
  const row = await waitFor(() => chatRow());
  expect(row.textContent).toContain('checksum mismatch');
  // Only the failed row says so.
  expect(screen.getByRole('group', { name: 'Модель пошуку' }).textContent).not.toContain('checksum');
  await fireEvent.click(inRow(row).button('Повторити'));
  expect(downloadModel).toHaveBeenCalledTimes(1);
  expect(downloadModel).toHaveBeenCalledWith('chat');
});

test('no space shows needed and free', async () => {
  await mountRows(ABSENT);
  downloadModel.mockRejectedValueOnce({ kind: 'noSpace', needed: 3.4e9, free: 1.2e9 });
  const row = await waitFor(() => chatRow());
  await fireEvent.click(inRow(row).button('Завантажити'));
  // `download` has re-read `local_models` by now (absent, as the mock answers):
  // the reason stays.
  await waitFor(() => expect(localModels).toHaveBeenCalledTimes(2));
  expect(chatRow().textContent).toContain('Потрібно ~3,4 ГБ, вільно 1,2 ГБ');
  // The other row is untouched.
  expect(screen.getByRole('group', { name: 'Модель пошуку' }).textContent).not.toContain('Потрібно');
  // Retry takes the reason away for the length of the new attempt.
  downloadModel.mockImplementationOnce(() => new Promise(() => {}));
  await fireEvent.click(inRow(chatRow()).button('Повторити'));
  expect(downloadModel).toHaveBeenCalledTimes(2);
  await waitFor(() => expect(chatRow().textContent).not.toContain('Потрібно'));
});

test('green dot only when both ready', async () => {
  const dot = { name: 'Моделі Mnema завантажені' };
  await mountRows(rows(READY, { kind: 'absent' }));
  await screen.findByRole('group', { name: 'Модель відповідей' });
  expect(screen.queryByRole('status', dot)).toBeNull();
  cleanup();
  await mountRows(rows({ kind: 'absent' }, READY));
  await screen.findByRole('group', { name: 'Модель відповідей' });
  expect(screen.queryByRole('status', dot)).toBeNull();
  cleanup();
  await mountRows(rows(READY, READY));
  expect(await screen.findByRole('status', dot)).toBeTruthy();
});

test('switching between bge-m3 providers asks no reindex', async () => {
  await mount({ settings: settings('baai/bge-m3', 120) });
  await fireEvent.click(await screen.findByRole('radio', MNEMA));
  await waitFor(() => expect(setProviderChoice).toHaveBeenCalledWith('mnema', 'keep'));
  expect(screen.queryByText('Змінити модель ембедингу?')).toBeNull();
});

test('switching from another embedding model asks first, and discards only on yes', async () => {
  await mount({ settings: settings('openai/text-embedding-3-small', 42) });
  await fireEvent.click(await screen.findByRole('radio', MNEMA));
  expect(await screen.findByText('Змінити модель ембедингу?')).toBeTruthy();
  // The existing count-based sentence, and the existing sentence about the loss.
  expect(screen.getByText(/42 ембединги в усіх векторних просторах/)).toBeTruthy();
  expect(screen.getByText(/зміна їх відкидає/)).toBeTruthy();
  // Nothing has been written, and the provider has not moved.
  expect(setProviderChoice).not.toHaveBeenCalled();
  // No is no.
  await fireEvent.click(screen.getByRole('button', { name: 'Не змінювати модель' }));
  expect(screen.queryByText('Змінити модель ембедингу?')).toBeNull();
  expect(setProviderChoice).not.toHaveBeenCalled();
  expect(screen.getByLabelText('Ключ:')).toBeTruthy();
  // Yes retires the old space, and says how much went.
  setProviderChoice.mockResolvedValueOnce({ choice: 'mnema', retired: [{ spaceId: 1, embeddedChunks: 42 }] });
  await fireEvent.click(screen.getByRole('radio', MNEMA));
  await fireEvent.click(await screen.findByRole('button', { name: 'Відкинути ембединги' }));
  await waitFor(() => expect(setProviderChoice).toHaveBeenCalledWith('mnema', 'discard'));
  expect(await screen.findByText('Зміна відкинула 42 ембединги.')).toBeTruthy();
  expect(screen.queryByLabelText('Ключ:')).toBeNull();
});

test('cancelling one of two downloads names only that model', async () => {
  await mountRows(ABSENT);
  downloadModel.mockImplementation(() => new Promise(() => {}));
  const embed = await screen.findByRole('group', { name: 'Модель пошуку' });
  await fireEvent.click(inRow(embed).button('Завантажити'));
  await fireEvent.click(inRow(chatRow()).button('Завантажити'));
  await fireEvent.click(inRow(chatRow()).button('Скасувати'));
  expect(cancelDownload).toHaveBeenCalledTimes(1);
  expect(cancelDownload).toHaveBeenCalledWith('chat');
  // The embed row is still downloading: it still offers to be cancelled.
  expect(inRow(embed).button('Скасувати')).toBeTruthy();
});
