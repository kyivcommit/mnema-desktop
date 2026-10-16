import { ROLE_FIELDS } from '../lib/fixtures';
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { expect, test, vi, beforeEach, afterEach } from 'vitest';
import Provider from './Provider.svelte';
import { setLocale } from '../i18n';
import type {
  LocalModelProgress, LocalModelRow, ModelSettings, ProviderChoice,
} from '../lib/ipc';

const mnemaAvailable = vi.fn();
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

function settings(embeddingModel: string | null, everywhere: number, over: Partial<ModelSettings> = {}): ModelSettings {
  return {
    key: { kind: 'absent' },
    index: {
      kind: 'read', embeddingModel, embeddedChunks: everywhere, embeddedChunksEverywhere: everywhere,
      totalChunks: everywhere, failedChunks: 0, pendingChunks: 0, indexedFiles: 1,
      lastIndexedAt: null, scanIncomplete: false, searchTextArm: true, searchContentArm: true,
    },
    platform: 'mac',
    ...ROLE_FIELDS,
    ...over,
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
  localModels.mockResolvedValue(over.rows ?? ABSENT);
  modelSettings.mockResolvedValue(over.settings ?? settings('baai/bge-m3', 0, over.choice ? { roles: { embed: over.choice, chat: over.choice } } : {}));
  const view = render(Provider, { children: keyField });
  await settle();
  return view;
}

Element.prototype.scrollIntoView = vi.fn();
const MNEMA = 'Mnema (локально)';
const picker = () => screen.getByRole('combobox', { name: 'Провайдер:' });
async function pick(name: string) {
  await fireEvent.click(picker());
  await fireEvent.click(screen.getByRole('option', { name: new RegExp(name) }));
}
const dotOf = (name: string) => {
  const o = screen.getAllByRole('option').find((x) => x.textContent?.includes(name))!;
  return o.querySelector('[data-configured]')!.getAttribute('data-configured');
};

beforeEach(() => {
  for (const f of [mnemaAvailable, localModels, downloadModel,
    cancelDownload, removeModel, modelSettings, unlisten]) f.mockReset();
  deliver = null;
  downloadModel.mockResolvedValue(undefined);
  cancelDownload.mockResolvedValue(undefined);
  removeModel.mockResolvedValue(undefined);
  setLocale('uk');
});
afterEach(() => { cleanup(); setLocale('en'); });

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

test('a download inherited from an earlier window is re-read after Cancel', async () => {
  const downloading = { kind: 'downloading', done: 10, total: 100 } as const;
  await mountRows(rows({ kind: 'absent' }, downloading));
  const row = await waitFor(() => chatRow());
  expect(within(row).getByRole('progressbar')).toBeTruthy();
  // The backend has not yet noticed the cancel: the bar stays, truthfully.
  localModels.mockResolvedValue(rows({ kind: 'absent' }, downloading));
  await fireEvent.click(inRow(row).button('Скасувати'));
  expect(cancelDownload).toHaveBeenCalledWith('chat');
  await waitFor(() => expect(localModels).toHaveBeenCalledTimes(2));
  expect(within(chatRow()).getByRole('progressbar')).toBeTruthy();
  // Once it has, the row is a plain Absent row again.
  localModels.mockResolvedValue(ABSENT);
  await fireEvent.click(inRow(chatRow()).button('Скасувати'));
  await waitFor(() => expect(inRow(chatRow()).button('Завантажити')).toBeTruthy());
  expect(within(chatRow()).queryByRole('progressbar')).toBeNull();
});

test('a cancel the backend refuses to take is not an unhandled rejection', async () => {
  await mountRows(rows({ kind: 'absent' }, { kind: 'downloading', done: 1, total: 2 }));
  cancelDownload.mockRejectedValueOnce(new Error('no such download'));
  await fireEvent.click(inRow(await waitFor(() => chatRow())).button('Скасувати'));
  expect(await screen.findByRole('alert')).toBeTruthy();
});

test('the dot follows configured', async () => {
  await mount({ settings: settings('baai/bge-m3', 0, { configured: { openRouter: true, mnemaEmbed: false, mnemaChat: false } }) });
  await fireEvent.click(picker());
  expect(dotOf('OpenRouter')).toBe('true');
  expect(dotOf('Mnema')).toBe('false');
  cleanup();
  await mount({ settings: settings('baai/bge-m3', 0, { configured: { openRouter: false, mnemaEmbed: false, mnemaChat: true } }) });
  await fireEvent.click(picker());
  expect(dotOf('OpenRouter')).toBe('false');
  expect(dotOf('Mnema')).toBe('true');
});

test('the dot has a spoken state too', async () => {
  await mount();
  await fireEvent.click(picker());
  const o = screen.getAllByRole('option').find((x) => x.textContent?.includes('OpenRouter'))!;
  expect(o.textContent).toMatch(/^\s*OpenRouter\s*, Налаштовано$/); // name first, state after it
  expect(screen.getAllByRole('option').find((x) => x.textContent?.includes('Mnema'))!.textContent).toContain('Не налаштовано');
});

test('mnema rows render inside the card', async () => {
  await mount();
  expect(screen.queryByRole('group', { name: 'Модель відповідей' })).toBeNull();
  await pick('Mnema');
  const card = await screen.findByTestId('provider-card');
  expect(within(card).getByRole('group', { name: 'Модель пошуку' })).toBeTruthy();
  expect(within(card).getByRole('group', { name: 'Модель відповідей' })).toBeTruthy();
  expect(within(card).getByText("~3,1 ГБ диска, ~4 ГБ пам'яті")).toBeTruthy();
});

test('choosing a provider writes nothing', async () => {
  await mount();
  await pick('Mnema');
  await pick('OpenRouter');
  for (const f of [downloadModel, cancelDownload, removeModel]) expect(f).not.toHaveBeenCalled();
  expect(modelSettings).toHaveBeenCalledTimes(1);
});

test('the card starts on the chat role provider', async () => {
  await mount({ choice: 'mnema' });
  expect(await screen.findByTestId('provider-card')).toBeTruthy();
  expect(picker().textContent).toContain('Mnema');
});

test('Mnema is offered only when available', async () => {
  await mount({ available: false });
  expect((picker() as HTMLButtonElement).disabled).toBe(true);
  cleanup();
  await mount({ available: true });
  await fireEvent.click(picker());
  expect(screen.getAllByRole('option')).toHaveLength(2);
});

test('the dot follows the name, in the closed button and in the list', async () => {
  await mountRows(ABSENT);
  await waitFor(() => picker());
  const after = (host: Element) => {
    const label = host.querySelector('.picker-label')!;
    const dot = host.querySelector('.mdot')!;
    return !!(label.compareDocumentPosition(dot) & Node.DOCUMENT_POSITION_FOLLOWING);
  };
  expect(after(picker())).toBe(true);
  await fireEvent.click(picker());
  for (const o of screen.getAllByRole('option')) expect(after(o)).toBe(true);
});
