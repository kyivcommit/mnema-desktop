export type Key = 'pin' | 'settings_title' | 'indexed_documents'
  | 'settings_nav_models' | 'settings_nav_folders' | 'settings_nav_indexing' | 'settings_nav_application'
  | 'settings_section_not_ready'
  | 'models_provider_label' | 'models_provider_name'
  | 'models_key_label' | 'models_key_saved' | 'models_key_absent_hint'
  | 'models_key_change' | 'models_key_forget' | 'models_key_save' | 'models_key_cancel'
  | 'models_key_removed' | 'models_key_nothing_to_remove'
  | 'models_key_locked' | 'models_key_duplicate' | 'models_key_refused' | 'models_key_defect'
  | 'models_index_not_open' | 'models_index_read_failed'
  | 'models_mac_keychain_note' | 'models_load_failed'
  | 'models_index_label'
  | 'models_tab_embedding' | 'models_tab_chat'
  | 'models_status_ready' | 'models_status_not_ready'
  | 'models_catalogue_empty' | 'models_catalogue_unreadable'
  | 'models_refusal_input_too_small' | 'models_refusal_no_stated_limit'
  | 'models_refusal_limit_not_understood' | 'models_refusal_no_stated_output_modalities'
  | 'models_refusal_no_text_output'
  | 'models_catalogue_unreadable_record_absent' | 'models_catalogue_unreadable_record_not_a_string'
  | 'models_catalogue_unreadable_record_known'
  | 'refusal_no_candidates' | 'refusal_empty_completion'
  | 'loc_page' | 'loc_line_one' | 'loc_line_many'
  | 'loc_row_one' | 'loc_row_many' | 'loc_sheet'
  | 'search_placeholder' | 'query_blank' | 'query_too_long' | 'query_failed'
  | 'phase_text' | 'phase_content' | 'phase_chat'
  | 'arm_text' | 'arm_content'
  | 'card_tree' | 'card_answer' | 'card_source'
  | 'no_path_on_disk' | 'answer_heading' | 'citations_heading'
  | 'tree_tab_files' | 'tree_tab_recents' | 'tree_empty' | 'tree_failed'
  | 'fresh_current' | 'fresh_reindexed' | 'fresh_file_changed'
  | 'fresh_file_missing' | 'fresh_no_path'
  | 'gone_no_such_chunk' | 'gone_id_reused'
  | 'source_loading' | 'source_failed' | 'source_wrong_document'
  | 'card_passages'
  | 'citations_only_banner' | 'citations_only_banner_empty' | 'citations_only_empty'
  | 'recent_now' | 'recent_minutes' | 'recent_hours' | 'recent_days';

export const messages: Record<'uk' | 'en', Record<Key, string>> = {
  uk: {
    pin: 'Пін',
    settings_title: 'Налаштування',
    settings_nav_models: 'Моделі',
    settings_nav_folders: 'Теки',
    settings_nav_indexing: 'Індексація',
    settings_nav_application: 'Застосунок',
    // Shared by both unbuilt sections — one sentence, promises nothing.
    settings_section_not_ready: 'Ця секція ще не готова.',
    // §9.1 / Task 4. Provider is a fixed, disabled control (v1 = OpenRouter
    // only, §4.4) — the name is a catalogue string, not a hardcoded literal,
    // because it is what a person reads there, not a testid.
    models_provider_label: 'Провайдер',
    models_provider_name: 'OpenRouter',
    models_key_label: 'Ключ',
    // Not a mask. models.rs:150-162 makes the key pub(crate) and never a
    // command, so the reply carries none and nothing here knows how long the
    // stored key is — a fixed run of dots would state a length this window
    // cannot know, and a screen reader would read it out one bullet at a time.
    // Present is a fact about the store (models.rs:676-679), so it is stated in
    // words.
    models_key_saved: 'Ключ збережено.',
    // Absent is the state of an application nobody has signed into
    // (models.rs:680-684). It says what the key is for and where it comes from,
    // and claims nothing about what happens after it is saved — the balance
    // KeyStatus carries is deliberately not rendered in this PR.
    models_key_absent_hint: 'Ключ OpenRouter потрібен, щоб застосунок міг звертатися до моделей. Створіть його в обліковому записі OpenRouter і вставте сюди.',
    models_key_change: 'Змінити',
    models_key_forget: 'Забути',
    models_key_save: 'Зберегти',
    models_key_cancel: 'Скасувати',
    // KeyRemoval's two answers (models.rs:101-108) — not the same sentence:
    // NothingToRemove is not a failure and is not "the key was removed" either.
    models_key_removed: 'Ключ видалено.',
    models_key_nothing_to_remove: 'Ключа й так не було.',
    // KeyStoreFailure's four causes (models.rs:718-746), each naming the one
    // action its own doc comment names — never `reason`, which stays out of
    // this screen entirely. Locked stands for two situations and claims
    // neither; Refused is the one value with no action to name.
    //
    // Locked renders no controls at all — offering to add or forget a key would
    // claim the store said something it did not — so its sentence is the only
    // thing a person has to act on, and it must name an action rather than
    // describe a state. It names both moves without asserting which situation
    // happened: models.rs:723-737 records that this build genuinely cannot tell
    // a locked store from a declined prompt, and that the earlier doc claiming
    // only "a locked keychain" was falsified by measurement.
    models_key_locked: 'Сховище ключів не відповіло. Розблокуйте його або дозвольте доступ, коли система про це запитає, і відкрийте це вікно знову.',
    models_key_duplicate: 'Під іменем цієї інсталяції збережено кілька ключів. Видаліть зайвий у системному сховищі.',
    models_key_refused: 'Сховище ключів відповіло відмовою. Ця збірка не може визначити, що робити далі.',
    models_key_defect: 'Це вада цієї збірки, а не стан вашої системи. Повідомте про неї розробникам.',
    // UnreadableCause's two values (models.rs:826-843) — NotOpen and
    // ReadFailed both leave `IndexSettings::Unreadable` with no `IndexRead` to
    // show, so this is the one sentence the section renders on that branch.
    models_index_not_open: 'Індекс ще не відкрито.',
    models_index_read_failed: 'Не вдалося прочитати індекс — це вада цієї збірки.',
    // Platform note (models.rs:606-627) — macOS only; Windows and Linux show
    // nothing here, because the same sentence would be noise on them.
    models_mac_keychain_note: 'Кожне оновлення застосунку робить його чужим для збереженого ключа: система один раз попросить пароль від зв’язки ключів для входу.',
    // The lead-in for a rejected read of `model_settings`. The rejection's own
    // sentence is shown verbatim beside it and never branched on (§10): a
    // rejection arrives as text, so this names what failed and the backend says
    // why.
    models_load_failed: 'Не вдалося прочитати налаштування моделей.',
    // Task 5 — the subject header the index sentence lacked: Task 4's review
    // found "Провайдер / [index sentence] / [mac note] / [key sentence]"
    // unreadable as a person, because nothing said the second line was about
    // the index. Shown only alongside that sentence, never on its own.
    models_index_label: 'Індекс',
    models_tab_embedding: 'Ембединг',
    models_tab_chat: 'Чат',
    // The green-dot rule (§9.1 / the PR 3 ruling `providerReady` already
    // carries): provider + key + a chosen embedding model, fail-safe on
    // anything missing. Named sentences rather than a bare state, so a screen
    // reader announces the same thing a sighted person reads.
    models_status_ready: 'Підключено — OpenRouter, ключ і обрана модель embedding готові.',
    models_status_not_ready: 'Ще не підключено — додайте ключ і оберіть модель embedding, щоб увімкнути пошук за змістом.',
    // An empty-but-well-formed catalogue (`models.rs:186-190`) is a stated
    // fact about the provider, not a failure of this build — said once, so a
    // person does not read a blank tab as a bug.
    models_catalogue_empty: 'Постачальник наразі не пропонує жодної моделі для цієї ролі.',
    // `Catalogue.unreadable` (catalogue.rs:246-257): a stated zero is never a
    // promise the list is complete on its own — this sentence is the promise,
    // and it is absent exactly when the count is zero.
    models_catalogue_unreadable: '{count, plural, one {# запис не вдалося прочитати} few {# записи не вдалося прочитати} many {# записів не вдалося прочитати} other {# записів не вдалося прочитати}}.',
    // `Refusal`'s five variants (catalogue.rs) — one sentence each, fixed
    // catalogue text rather than the provider's own words: see
    // `Models.svelte`'s `refusalReason` for why `raw` is not interpolated
    // into the three variants that carry it.
    models_refusal_input_too_small: 'Ця модель заявляє ліміт входу {limit} токенів — менше за поріг {floor}, потрібний цій програмі.',
    models_refusal_no_stated_limit: 'Постачальник не вказує ліміт входу цієї моделі.',
    models_refusal_limit_not_understood: 'Постачальник вказує ліміт входу у форматі, який ця збірка не вміє прочитати.',
    models_refusal_no_stated_output_modalities: 'Постачальник не вказує, що видає ця модель.',
    models_refusal_no_text_output: 'Постачальник заявляє, що ця модель не видає текст.',
    // `RecordId`'s three states (catalogue.rs:293-304) — a record that never
    // became a model still gets one line naming its position, so "N records
    // unreadable" points at something (Task 2 review, item 4).
    models_catalogue_unreadable_record_absent: 'Запис на позиції {index}: постачальник не вказав ідентифікатор моделі.',
    models_catalogue_unreadable_record_not_a_string: 'Запис на позиції {index}: ідентифікатор моделі не був текстом.',
    models_catalogue_unreadable_record_known: 'Запис на позиції {index}, ідентифікатор «{id}»: решту запису ця збірка прочитати не змогла.',
    indexed_documents: '{count, plural, one {# документ} few {# документи} many {# документів} other {# документа}}',
    refusal_no_candidates: 'Нічого не знайдено за цим запитом.',
    refusal_empty_completion: 'Модель не повернула відповіді.',
    loc_page: 'с.', loc_line_one: 'рядок', loc_line_many: 'рядки',
    loc_row_one: 'рядок', loc_row_many: 'рядки', loc_sheet: 'аркуш',
    search_placeholder: 'Запит…',
    query_blank: 'Введіть запит.',
    query_too_long: 'Запит задовгий (максимум {limit} символів).',
    query_failed: 'Не вдалося виконати запит.',
    phase_text: 'текст', phase_content: 'зміст', phase_chat: 'чат',
    arm_text: 'текст', arm_content: 'зміст',
    card_tree: 'Дерево', card_answer: 'Відповідь', card_source: 'Джерело',
    // Ruling on the state E aria-label: the centre card is one <section>, but
    // it is not one FACT. Announcing state E as «Відповідь» named the region
    // for the thing it says is unavailable, where a person using a screen
    // reader cannot see the contradiction and correct for it.
    card_passages: 'Уривки',
    no_path_on_disk: 'нема на диску', answer_heading: 'Відповідь', citations_heading: 'Цитати',
    tree_tab_files: 'Файли', tree_tab_recents: 'Нещодавні',
    tree_empty: 'Ще нічого не проіндексовано.', tree_failed: 'Не вдалося завантажити дерево.',
    fresh_current: 'Актуально',
    fresh_reindexed: 'Цей шлях тепер належить іншому документу',
    fresh_file_changed: 'Файл змінився після індексації',
    fresh_file_missing: 'Файла немає на диску',
    // Ruling X: `noPath` has three causes and deletion is only one of them
    // (`src-tauri/src/tree.rs:226-241`), so this says the location is unknown
    // and never that the file is gone.
    fresh_no_path: 'Розташування на диску невідоме',
    gone_no_such_chunk: 'Цього фрагмента більше немає в індексі',
    gone_id_reused: 'Цей ідентифікатор тепер вказує на інший фрагмент',
    source_loading: 'Завантаження джерела…',
    source_failed: 'Не вдалося завантажити джерело.',
    // M2: shown instead of a freshness verdict when the excerpt names a
    // different document than the citation does.
    source_wrong_document: 'Цей уривок походить з іншого документа, ніж цитата',
    // 🔴 Ruling AF: `bridge.rs:536-540` opens state E for any non-`Ready`
    // readiness, `bridge.rs:293-302` gives that three variants, and the wire
    // shape at `bridge.rs:476-480` carries none of them. So this sentence says
    // only what the payload proves — no cause, and no instruction pointing at a
    // settings screen that might be the wrong one.
    //
    // 🔴 Review I1: TWO forms, and the second is not a duplicate. The first
    // clause is true in both; the second one — «нижче — уривки» — is a promise
    // about what follows, and with zero hits it was printed directly above
    // `citations_only_empty` denying it. A card contradicting itself is Ruling
    // AF's own failure one branch over, so the empty card drops the clause it
    // cannot keep rather than qualifying it.
    // Re-review RM1: ICU plural, the mechanism `indexed_documents` above already
    // uses. Ukrainian needs three arms an integer count can reach — `ASK_TOP_K` is
    // 8 (`bridge.rs:496`), so one/few/many are all states a person gets to — and a
    // fixed plural was ungrammatical over a single passage, not merely loose.
    citations_only_banner: 'Генерування недоступне. Пошук знайшов {count, plural, one {# уривок} few {# уривки} many {# уривків} other {# уривка}}.',
    citations_only_banner_empty: 'Генерування недоступне.',
    // Ruling AK: its own sentence, distinct from `tree_empty` (nothing indexed
    // at all) and from `source_failed` (a passage that could not be read).
    citations_only_empty: 'Жоден уривок не відповідає цьому запиту.',
    // Review Minor 5: the Recents tab renders WHEN each document was indexed,
    // and the wire carries it (`ipc.ts:65`, seconds since the epoch —
    // `schema.sql:261`'s `unixepoch()`). Relative rather than a date, and that
    // is a decision: a formatted date needs a time zone, which makes what a
    // person sees depend on the machine the card runs on, while "how long ago"
    // is the question the card's own name asks and needs no zone at all.
    // The plural arms are the mechanism `indexed_documents` already uses;
    // Ukrainian takes the accusative after «тому».
    recent_now: 'щойно',
    recent_minutes: '{count, plural, one {# хвилину} few {# хвилини} many {# хвилин} other {# хвилини}} тому',
    recent_hours: '{count, plural, one {# годину} few {# години} many {# годин} other {# години}} тому',
    recent_days: '{count, plural, one {# день} few {# дні} many {# днів} other {# дня}} тому',
  },
  en: {
    pin: 'Pin',
    settings_title: 'Settings',
    settings_nav_models: 'Models',
    settings_nav_folders: 'Folders',
    settings_nav_indexing: 'Indexing',
    settings_nav_application: 'Application',
    settings_section_not_ready: 'This section is not ready yet.',
    models_provider_label: 'Provider',
    models_provider_name: 'OpenRouter',
    models_key_label: 'Key',
    models_key_saved: 'A key is saved.',
    models_key_absent_hint: 'An OpenRouter key lets this application reach the models. Create one in your OpenRouter account and paste it here.',
    models_key_change: 'Change',
    models_key_forget: 'Forget',
    models_key_save: 'Save',
    models_key_cancel: 'Cancel',
    models_key_removed: 'The key was removed.',
    models_key_nothing_to_remove: 'There was no key to remove.',
    models_key_locked: 'The credential store did not answer. Unlock it, or allow access when the system asks for it, then open this window again.',
    models_key_duplicate: 'More than one credential is filed under this installation. Remove the duplicate in the system credential store.',
    models_key_refused: 'The credential store refused to answer. This build cannot tell what to do next.',
    models_key_defect: 'This is a defect in this build, not a state of your system. Please report it to the developers.',
    models_index_not_open: 'The index is not open yet.',
    models_index_read_failed: 'The index could not be read — this is a defect in this build.',
    models_mac_keychain_note: 'Every update makes this application a stranger to its own key: the system will ask once for your login keychain password.',
    models_load_failed: 'The model settings could not be read.',
    models_index_label: 'Index',
    models_tab_embedding: 'Embedding',
    models_tab_chat: 'Chat',
    models_status_ready: 'Connected — OpenRouter, a key and a chosen embedding model are all set.',
    models_status_not_ready: 'Not connected yet — add a key and choose an embedding model to enable content search.',
    models_catalogue_empty: 'The provider does not currently list any models for this role.',
    models_catalogue_unreadable: '{count, plural, one {# record could not be read} other {# records could not be read}}.',
    models_refusal_input_too_small: 'This model states an input limit of {limit} tokens, under the {floor} this application requires.',
    models_refusal_no_stated_limit: 'The provider does not state an input limit for this model.',
    models_refusal_limit_not_understood: 'The provider states an input limit in a shape this build cannot read.',
    models_refusal_no_stated_output_modalities: 'The provider does not state what this model outputs.',
    models_refusal_no_text_output: 'The provider states that this model does not output text.',
    models_catalogue_unreadable_record_absent: 'Record at position {index}: the provider stated no model id.',
    models_catalogue_unreadable_record_not_a_string: 'Record at position {index}: the model id was not text.',
    models_catalogue_unreadable_record_known: 'Record at position {index}, id "{id}": this build could not read the rest of the record.',
    indexed_documents: '{count, plural, one {# document} other {# documents}}',
    refusal_no_candidates: 'Nothing was found for this query.',
    refusal_empty_completion: 'The model returned no answer.',
    loc_page: 'p.', loc_line_one: 'line', loc_line_many: 'lines',
    loc_row_one: 'row', loc_row_many: 'rows', loc_sheet: 'sheet',
    search_placeholder: 'Query…',
    query_blank: 'Enter a query.',
    query_too_long: 'The query is too long (max {limit} characters).',
    query_failed: 'The query could not be run.',
    phase_text: 'text', phase_content: 'content', phase_chat: 'chat',
    arm_text: 'text', arm_content: 'content',
    card_tree: 'Tree', card_answer: 'Answer', card_source: 'Source',
    card_passages: 'Passages',
    no_path_on_disk: 'no path on disk', answer_heading: 'Answer', citations_heading: 'Citations',
    tree_tab_files: 'Files', tree_tab_recents: 'Recents',
    tree_empty: 'Nothing is indexed yet.', tree_failed: 'The tree could not be loaded.',
    fresh_current: 'Up to date',
    fresh_reindexed: 'This path now names another document',
    fresh_file_changed: 'The file changed after indexing',
    fresh_file_missing: 'The file is missing from disk',
    fresh_no_path: 'The location on disk is unknown',
    gone_no_such_chunk: 'This passage is no longer in the index',
    gone_id_reused: 'This identifier now points to another passage',
    source_loading: 'Loading the source…',
    source_failed: 'The source could not be loaded.',
    source_wrong_document: 'This excerpt came from a different document than the citation',
    citations_only_banner: 'Generation is unavailable. The search found {count, plural, one {# passage} other {# passages}}.',
    citations_only_banner_empty: 'Generation is unavailable.',
    citations_only_empty: 'No passages matched this query.',
    recent_now: 'just now',
    recent_minutes: '{count, plural, one {# minute} other {# minutes}} ago',
    recent_hours: '{count, plural, one {# hour} other {# hours}} ago',
    recent_days: '{count, plural, one {# day} other {# days}} ago',
  },
};
