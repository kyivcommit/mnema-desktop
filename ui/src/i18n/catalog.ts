export type Key = 'pin' | 'settings_title' | 'indexed_documents'
  | 'settings_nav_models' | 'settings_nav_folders' | 'settings_nav_indexing' | 'settings_nav_application'
  | 'settings_section_not_ready'
  | 'settings_folders_empty' | 'settings_folders_add' | 'settings_folders_remove'
  | 'settings_folders_load_failed' | 'settings_folders_indexed' | 'settings_folders_remove_named'
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
  | 'models_refusal_no_text_output' | 'models_entry_reason_separator'
  | 'models_catalogue_unreadable_record_absent' | 'models_catalogue_unreadable_record_not_a_string'
  | 'models_catalogue_unreadable_record_known'
  | 'models_embedding_confirm_title' | 'models_embedding_confirm_estimate'
  | 'models_embedding_confirm_loss' | 'models_embedding_discard' | 'models_embedding_cancel'
  | 'models_embedding_retired' | 'models_embedding_retired_none'
  | 'models_embedding_degraded' | 'models_embedding_reembed' | 'models_embedding_reembed_started'
  | 'models_embedding_reembed_ended'
  | 'models_embedding_change_failed' | 'models_job_running' | 'models_index_recover'
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
  | 'settings_folders_scan' | 'settings_folders_scan_named'
  | 'settings_folders_expand' | 'settings_folders_expand_named'
  | 'settings_subfolders_loading' | 'settings_subfolders_none'
  | 'settings_subfolders_unnameable' | 'settings_subfolders_failed'
  | 'settings_subfolder_open' | 'settings_subfolder_excluded'
  | 'settings_subfolder_excluded_by_ancestor' | 'settings_subfolder_built_in'
  | 'settings_subfolder_symlink' | 'settings_subfolder_unusable_name'
  | 'settings_subfolder_exclude' | 'settings_subfolder_exclude_named'
  | 'settings_subfolder_include' | 'settings_subfolder_include_named'
  | 'settings_folders_rules_heading' | 'settings_folders_rules_none'
  | 'settings_folders_rule_gone' | 'settings_folders_rule_cost'
  | 'settings_folders_rule_remove' | 'settings_folders_rule_remove_named'
  | 'settings_folders_rule_already_gone'
  | 'indexing_walk_starting' | 'indexing_walk_running'
  | 'indexing_embed_starting' | 'indexing_embed_running'
  | 'indexing_counts_ratio' | 'indexing_counts_counting'
  | 'indexing_eta' | 'indexing_eta_unknown'
  | 'indexing_walk_ended_completed' | 'indexing_walk_ended_partly_read'
  | 'indexing_walk_ended_cancelled' | 'indexing_walk_ended_failed'
  | 'indexing_walk_ended_broken_worker' | 'indexing_walk_ended_rules_not_applied'
  | 'indexing_walk_ended_root_unavailable' | 'indexing_walk_ended_volume_missing'
  | 'indexing_embed_ended_completed' | 'indexing_embed_ended_cancelled'
  | 'indexing_embed_ended_failed' | 'indexing_embed_ended_unexpected'
  | 'indexing_failure_message' | 'indexing_walk_result' | 'indexing_embed_result'
  | 'indexing_frozen_heading' | 'indexing_frozen_row'
  | 'indexing_frozen_symlinked_subtree' | 'indexing_frozen_empty_directory'
  | 'indexing_frozen_unreadable_directory'
  | 'indexing_note_no_key' | 'indexing_note_no_model' | 'indexing_note_rejected'
  | 'indexing_unobserved' | 'indexing_cancel'
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
    // §9.2, Task 7. `TreeRoot` (ipc.ts) carries no flag for "walked and found
    // empty" vs. "not walked yet" — a folder just added and one genuinely
    // empty are the same value on the wire — so this sentence names only the
    // absence of watched folders, never a folder's own content. The per-row
    // count (`settings_folders_indexed`, below) carries the state a folder's
    // own row can actually prove. (Not "shared with the launcher tree" —
    // `git grep indexed_documents` shows no launcher component renders that
    // key, before or after this commit; P3-7 review.)
    settings_folders_empty: 'Ще жодної теки не додано.',
    settings_folders_add: 'Додати теку',
    settings_folders_remove: 'Видалити',
    // Lead-in for a rejected `list_tree` (§10: the rejection's own sentence is
    // shown verbatim beside this, never branched on).
    settings_folders_load_failed: 'Не вдалося прочитати список тек.',
    // §9.2 review (P2-4): the bare count read as a claim about the FOLDER
    // ("this folder has 0 documents"), forever, since the ruling defers the walk
    // that would ever change it. This key names the subject — the index, not
    // the folder — reusing `indexed_documents`'s own plural arms rather than
    // duplicating them (do not change that shared key: it is a different
    // sentence for a different place, §7.3/launcher `Tree.svelte`).
    settings_folders_indexed: '{count, plural, one {Проіндексовано: # документ} few {Проіндексовано: # документи} many {Проіндексовано: # документів} other {Проіндексовано: # документа}}',
    // §9.2 review (P2-5): two "Видалити" buttons in a two-folder list share
    // one accessible name. `aria-label` carries the folder's own path so a
    // screen reader distinguishes them; the visible button text stays plain
    // "Видалити" (settings_folders_remove, above).
    settings_folders_remove_named: 'Видалити {path}',
    // §9.1 / Task 4. Provider is a fixed, disabled control (v1 = OpenRouter
    // only, §4.4) — the name is a catalogue string, not a hardcoded literal,
    // because it is what a person reads there, not a testid.
    // 🔴 Live run, finding 1: a label and its value are two things, and this
    // window has no CSS to say so — it is not written yet, and it lands in a
    // later PR. Without a separator IN THE TEXT the screen read
    // «Провайдер OpenRouter» and «Ключ Ключ збережено.»: one broken phrase, and
    // it would stay one in every text-only rendering of this window — a screen
    // reader, a copy-paste, a plain-text export — long after styling arrives.
    // The colon lives inside each label rather than in a shared separator
    // string because punctuation after a label is a per-locale decision (French
    // puts a space before it) and because this catalogue already writes it that
    // way in `settings_folders_indexed`.
    models_provider_label: 'Провайдер:',
    models_provider_name: 'OpenRouter',
    models_key_label: 'Ключ:',
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
    models_index_label: 'Індекс:',
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
    // The same seam as the labels above, one row lower and never reached by the
    // live run: a greyed model's name and the sentence saying why it cannot be
    // chosen are two inline spans, so they read as «Назва моделі Постачальник не
    // вказує ліміт входу…» — one phrase. The dash belongs to neither of them, so
    // it is its own string rather than a prefix inside five reason sentences.
    models_entry_reason_separator: '—',
    // `RecordId`'s three states (catalogue.rs:293-304) — a record that never
    // became a model still gets one line naming its position, so "N records
    // unreadable" points at something (Task 2 review, item 4).
    models_catalogue_unreadable_record_absent: 'Запис на позиції {index}: постачальник не вказав ідентифікатор моделі.',
    models_catalogue_unreadable_record_not_a_string: 'Запис на позиції {index}: ідентифікатор моделі не був текстом.',
    models_catalogue_unreadable_record_known: 'Запис на позиції {index}, ідентифікатор «{id}»: решту запису ця збірка прочитати не змогла.',
    // §9.1 / Task 6 — обрання моделі ембедингу. Дві цифри про два різні
    // моменти, і вікно каже, яка з них яка: оцінка ДО дії читається з
    // `embeddedChunksEverywhere`, а скільки саме зникло — з `AdoptedModel.retired`,
    // виміряного в мить знищення.
    models_embedding_confirm_title: 'Змінити модель ембедингу?',
    models_embedding_confirm_estimate: 'Зараз індекс містить {count, plural, one {# ембединг} few {# ембединги} many {# ембедингів} other {# ембедингів}} в усіх векторних просторах. Це оцінка, зроблена до зміни; скільки саме було відкинуто, буде сказано після неї.',
    models_embedding_confirm_loss: 'Ці ембединги неможливо перенести: зміна їх відкидає. Пошук за змістом буде недоступний, доки індекс не буде вбудовано наново; пошук за словами працюватиме далі.',
    models_embedding_discard: 'Відкинути ембединги',
    models_embedding_cancel: 'Не змінювати модель',
    models_embedding_retired: 'Зміна відкинула {count, plural, one {# ембединг} few {# ембединги} many {# ембедингів} other {# ембедингів}} з {spaces, plural, one {# векторного простору} other {# векторних просторів}}.',
    models_embedding_retired_none: 'Зміна нічого не відкинула: жоден векторний простір їй не заважав.',
    models_embedding_degraded: 'Пошук за змістом недоступний, доки індекс не буде вбудовано наново. Пошук за словами працює далі.',
    models_embedding_reembed: 'Вбудувати індекс наново',
    models_embedding_reembed_started: 'Вбудовування почалося.',
    models_embedding_reembed_ended: 'Вбудовування завершилося, а пошук за змістом досі недоступний. Його можна запустити ще раз.',
    models_embedding_change_failed: 'Модель ембедингу не прийнято. Прочитайте повідомлення нижче: зміна, яка не завершилася, все одно могла відкинути ембединги.',
    models_job_running: 'Триває завдання індексації. Його не зупинено, воно працює далі.',
    models_index_recover: 'Повторний вибір моделі ембедингу це виправляє: він наново записує вказівник, який індекс втратив. Нічого з уже вбудованого при цьому не відкидається.',
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
    // §9.2 / Task 8 — running the index, showing it, stopping it. The scan
    // starts on this control and never on adding a folder, because excluding
    // subfolders (PR 8) is a move a person still has to make in between.
    settings_folders_scan: 'Сканувати',
    // Carries the path so two "Сканувати" buttons in a list stay apart for a
    // screen reader; the VISIBLE label stays the plain word above.
    settings_folders_scan_named: 'Сканувати {path}',
    // PR 8a, Task 5 — the folder row expands into what is on disk.
    //
    // The control keeps ONE name in both states: `aria-expanded` carries open
    // and shut, and a button whose label flips is a second place for the same
    // fact — the two can disagree, and only one of them is what a screen
    // reader announces.
    settings_folders_expand: 'Підтеки',
    settings_folders_expand_named: 'Підтеки теки {path}',
    settings_subfolders_loading: 'Читаємо підтеки…',
    settings_subfolders_none: 'У цій теці немає підтек.',
    // `unnameable` (tree.rs): записи, чиї назви не є коректним UTF-8, лічать і
    // не показують — назва, зіпсована при показі, більше не відкриває ту теку,
    // з якої походить, і правило з неї не виключило б нічого. Речення існує,
    // щоб тека з такими записами не читалась як порожніша, ніж вона є.
    settings_subfolders_unnameable: '{count, plural, one {# підтеку не показано: її назву не вдалося прочитати як текст.} few {# підтеки не показано: їхні назви не вдалося прочитати як текст.} many {# підтек не показано: їхні назви не вдалося прочитати як текст.} other {# підтеки не показано: їхні назви не вдалося прочитати як текст.}}',
    // Вступ до відмови `list_subfolders`; саме речення бекенда показують
    // дослівно поруч (§10).
    settings_subfolders_failed: 'Не вдалося прочитати підтеки цієї теки.',
    // Шість станів, шість речень. `open` не обіцяє індексування — команда знає
    // лише про правила, тож речення говорить саме про правила.
    settings_subfolder_open: 'Жодне правило не виключає цю теку.',
    settings_subfolder_excluded: 'Виключено вашим правилом.',
    // Називає предка: рядок «утримується правилом» без назви правила не
    // лишає людині нічого, що можна піти й прибрати.
    settings_subfolder_excluded_by_ancestor: 'Утримується вашим правилом на {prefix}. Приберіть те правило, щоб змінити цю теку.',
    // `built_in` і `unusable_name` — протилежні факти, і речення НЕ мають
    // читатись однаково: вміст першої не потрапляє до провайдера ніколи,
    // вміст другої потрапляє, і людина не може захистити її звідси.
    settings_subfolder_built_in: 'Застосунок ніколи не індексує цю теку, тож тут немає правила, яке можна додати чи прибрати.',
    settings_subfolder_symlink: 'Посилання на іншу теку. Сканування ніколи не переходить за посиланнями, тож усередині нічого не індексується.',
    settings_subfolder_unusable_name: 'Ця тека індексується, а її назву не можна записати як правило — перейменуйте теку, якщо хочете її виключити.',
    settings_subfolder_exclude: 'Виключити',
    settings_subfolder_exclude_named: 'Виключити {path}',
    settings_subfolder_include: 'Не виключати',
    settings_subfolder_include_named: 'Не виключати {path}',
    settings_folders_rules_heading: 'Ваші правила виключення для цієї теки:',
    settings_folders_rules_none: 'Ви нічого не виключили в цій теці.',
    // Єдине джерело відповіді «чи тека ще на диску» — `existsOnDisk` самого
    // правила (bridge.rs). Порівняння списку правил зі списком підтек одного
    // рівня помилково назве застарілим кожне вкладене правило.
    settings_folders_rule_gone: 'Наразі за цим шляхом теки немає.',
    // Прибрати правило — це розкриття, а не прибирання: речення стоїть поруч
    // із кнопкою ДО натискання й каже, що станеться далі. Одне речення для
    // обох місць — і для правила у списку, і для перемикача «не виключати».
    settings_folders_rule_cost: 'Без цього правила все за цим шляхом знову індексуватиметься від наступного сканування.',
    settings_folders_rule_remove: 'Прибрати правило',
    settings_folders_rule_remove_named: 'Прибрати правило на {prefix}',
    // `include_subfolder` відповідає, чи справді щось прибрали (bridge.rs).
    // «Правила вже не було» — не помилка, а факт про екран, який застарів.
    settings_folders_rule_already_gone: 'Такого правила вже не було. Список перечитано.',
    indexing_walk_starting: 'Читання теки починається…',
    indexing_walk_running: 'Триває читання теки.',
    // The embedding pass takes no root and covers the whole index
    // (embed_job.rs), so neither of these two may name the folder that was
    // pressed.
    indexing_embed_starting: 'Вбудовування всього індексу починається…',
    indexing_embed_running: 'Триває вбудовування всього індексу.',
    indexing_counts_ratio: 'Опрацьовано {done} з {total}. Пропущено: {skipped}. Відхилено: {refused}.',
    // `total: 0` is not an edge case: a walk reports it before phase 1 has
    // counted anything. "0 з 0" would read as "нема чого робити".
    indexing_counts_counting: 'Опрацьовано {done}. Скільки їх усього, поки не відомо. Пропущено: {skipped}. Відхилено: {refused}.',
    indexing_eta: 'Залишилось приблизно {seconds} с.',
    // `secondsLeft` is `Option<u64>`: "ще не відомо" is a real state, and it is
    // the ordinary one at the start of every run.
    indexing_eta_unknown: 'Скільки ще лишилось часу, поки не відомо.',
    indexing_walk_ended_completed: 'Теку прочитано повністю.',
    // `reason: completed` with `complete: false` (job.rs): phase 1 never saw
    // the whole tree, so what was deleted under an unreadable subfolder is
    // still searchable. That is why the word "done" cannot appear here.
    indexing_walk_ended_partly_read: 'Теку прочитано лише частково: до якихось підтек не вдалося зайти. Файли, які ви вилучили всередині них, досі знаходяться пошуком.',
    indexing_walk_ended_cancelled: 'Сканування зупинено на ваше прохання.',
    indexing_walk_ended_failed: 'Сканування обірвалося через збій.',
    // The four sentences below are not about a malfunction: they are decisions
    // the walk itself made (job.rs), and calling them a failure would tell a
    // person the program broke when instead a folder cannot be read.
    indexing_walk_ended_broken_worker: 'Сканування спинилося: допоміжна програма, яка читає файли, перестала відповідати.',
    indexing_walk_ended_rules_not_applied: 'Сканування спинилося: правила виключення не вдалося застосувати, тож теку не читали зовсім.',
    indexing_walk_ended_root_unavailable: 'Сканування спинилося: у теку не вдалося зайти. Можливо, її прибрали або диск від’єднано.',
    indexing_walk_ended_volume_missing: 'Сканування спинилося: тека прочиталася порожньою, хоча в індексі є файли з неї. Нічого не вилучено — можливо, диск під’єднано не повністю.',
    indexing_embed_ended_completed: 'Вбудовування всього індексу завершено.',
    indexing_embed_ended_cancelled: 'Вбудовування зупинено на ваше прохання.',
    indexing_embed_ended_failed: 'Вбудовування обірвалося через збій.',
    // The walk is the only writer of the four `StopReason` reasons (job.rs), so
    // they cannot reach the embedding pass. A sentence exists for them anyway,
    // carrying the state's own name: a default branch that draws "finished" is
    // exactly how a failed pass reads as a successful one.
    indexing_embed_ended_unexpected: 'Вбудовування спинилося з причини, якої тут не очікували ({reason}).',
    indexing_failure_message: 'Програма повідомила: {message}',
    indexing_walk_result: 'Додано документів: {indexed}. Без змін: {unchanged}. Пропущено: {skipped}. Вилучено з індексу: {removed}.',
    indexing_embed_result: 'Вбудовано фрагментів: {done} з {total}. Відхилено: {refused}.',
    // `frozen` is shown, not dropped: `removed: 0` alone cannot say whether
    // anything was silently left untouched (job.rs).
    indexing_frozen_heading: 'Ці підтеки не звіряли, тож вилучені з них файли досі знаходяться пошуком:',
    indexing_frozen_row: '{prefix} — {why}',
    indexing_frozen_symlinked_subtree: 'символьне посилання, сюди не заходили',
    indexing_frozen_empty_directory: 'прочиталася порожньою',
    indexing_frozen_unreadable_directory: 'не вдалося прочитати',
    // The walk runs regardless, because word search needs neither a key
    // nor a model — so each sentence names what is absent and what already
    // works.
    indexing_note_no_key: 'Пошук за змістом не вмикали: ключ провайдера не збережено. Пошук по словах у цій теці вже працює.',
    indexing_note_no_model: 'Пошук за змістом не вмикали: модель вбудовування не обрана. Пошук по словах у цій теці вже працює.',
    indexing_note_rejected: 'Запит відхилено.',
    indexing_unobserved: 'Зараз виконується інше завдання. Це вікно не бачить, як далеко воно просунулося, але зупинити його можна.',
    indexing_cancel: 'Зупинити',
  },
  en: {
    pin: 'Pin',
    settings_title: 'Settings',
    settings_nav_models: 'Models',
    settings_nav_folders: 'Folders',
    settings_nav_indexing: 'Indexing',
    settings_nav_application: 'Application',
    settings_section_not_ready: 'This section is not ready yet.',
    settings_folders_empty: 'No folder has been added yet.',
    settings_folders_add: 'Add a folder',
    settings_folders_remove: 'Remove',
    settings_folders_load_failed: 'The list of folders could not be read.',
    settings_folders_indexed: '{count, plural, one {Indexed: # document} other {Indexed: # documents}}',
    settings_folders_remove_named: 'Remove {path}',
    models_provider_label: 'Provider:',
    models_provider_name: 'OpenRouter',
    models_key_label: 'Key:',
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
    models_index_label: 'Index:',
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
    models_entry_reason_separator: '—',
    models_catalogue_unreadable_record_absent: 'Record at position {index}: the provider stated no model id.',
    models_catalogue_unreadable_record_not_a_string: 'Record at position {index}: the model id was not text.',
    models_catalogue_unreadable_record_known: 'Record at position {index}, id "{id}": this build could not read the rest of the record.',
    models_embedding_confirm_title: 'Change the embedding model?',
    models_embedding_confirm_estimate: 'The index holds {count, plural, one {# embedding} other {# embeddings}} across all its vector spaces right now. That is an estimate read before the change; what the change actually discarded is reported after it.',
    models_embedding_confirm_loss: 'These embeddings cannot be carried over: the change discards them. Search by meaning will be unavailable until the index is embedded again; search by words will still answer.',
    models_embedding_discard: 'Discard the embeddings',
    models_embedding_cancel: 'Do not change the model',
    models_embedding_retired: 'The change discarded {count, plural, one {# embedding} other {# embeddings}} from {spaces, plural, one {# vector space} other {# vector spaces}}.',
    models_embedding_retired_none: 'The change discarded nothing: no vector space was in its way.',
    models_embedding_degraded: 'Search by meaning is unavailable until the index is embedded again. Search by words still answers.',
    models_embedding_reembed: 'Embed the index again',
    models_embedding_reembed_started: 'Embedding has started.',
    models_embedding_reembed_ended: 'The embedding pass has ended, and search by meaning is still unavailable. It can be started again.',
    models_embedding_change_failed: 'The embedding model was not adopted. Read the message below — a change that fails partway can still have discarded embeddings.',
    models_job_running: 'An indexing job is running. It was not stopped, and it is still going.',
    models_index_recover: 'Choosing an embedding model again repairs this: it rewrites the pointer the index lost. Nothing already embedded is discarded by it.',
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
    settings_folders_scan: 'Scan',
    settings_folders_scan_named: 'Scan {path}',
    settings_folders_expand: 'Subfolders',
    settings_folders_expand_named: 'Subfolders of {path}',
    settings_subfolders_loading: 'Reading the subfolders…',
    settings_subfolders_none: 'This folder has no subfolders.',
    settings_subfolders_unnameable: '{count, plural, one {# subfolder is not listed: its name could not be read as text.} other {# subfolders are not listed: their names could not be read as text.}}',
    settings_subfolders_failed: 'The subfolders of this folder could not be read.',
    settings_subfolder_open: 'No rule excludes this folder.',
    settings_subfolder_excluded: 'Excluded by your rule.',
    settings_subfolder_excluded_by_ancestor: 'Held by your rule on {prefix}. Remove that rule to change this folder.',
    settings_subfolder_built_in: 'The application never indexes this folder, so there is no rule to add or remove.',
    settings_subfolder_symlink: 'A link to another folder. The scan never follows links, so nothing inside it is indexed.',
    settings_subfolder_unusable_name: 'This folder is indexed, and its name cannot be written as a rule here — rename it if you need to exclude it.',
    settings_subfolder_exclude: 'Exclude',
    settings_subfolder_exclude_named: 'Exclude {path}',
    settings_subfolder_include: 'Do not exclude',
    settings_subfolder_include_named: 'Do not exclude {path}',
    settings_folders_rules_heading: 'Your exclusion rules for this folder:',
    settings_folders_rules_none: 'You have not excluded anything in this folder.',
    settings_folders_rule_gone: 'There is no folder at this path right now.',
    settings_folders_rule_cost: 'Without this rule, anything at this path is indexed again from the next scan on.',
    settings_folders_rule_remove: 'Remove the rule',
    settings_folders_rule_remove_named: 'Remove the rule on {prefix}',
    settings_folders_rule_already_gone: 'There was no such rule left to remove. The list has been re-read.',
    indexing_walk_starting: 'Reading the folder is starting…',
    indexing_walk_running: 'The folder is being read.',
    indexing_embed_starting: 'Embedding the whole index is starting…',
    indexing_embed_running: 'The whole index is being embedded.',
    indexing_counts_ratio: 'Processed {done} of {total}. Skipped: {skipped}. Given up on: {refused}.',
    indexing_counts_counting: 'Processed {done}. How many there are in total is not known yet. Skipped: {skipped}. Given up on: {refused}.',
    indexing_eta: 'About {seconds} s left.',
    indexing_eta_unknown: 'How much time is left is not known yet.',
    indexing_walk_ended_completed: 'The folder was read in full.',
    indexing_walk_ended_partly_read: 'The folder was only partly read: some subfolders could not be entered. Files you deleted inside them are still found by search.',
    indexing_walk_ended_cancelled: 'The scan was stopped at your request.',
    indexing_walk_ended_failed: 'The scan broke off because something went wrong.',
    indexing_walk_ended_broken_worker: 'The scan stopped: the helper program that reads files stopped answering.',
    indexing_walk_ended_rules_not_applied: 'The scan stopped: the exclusion rules could not be applied, so the folder was not read at all.',
    indexing_walk_ended_root_unavailable: 'The scan stopped: the folder could not be entered. It may have been removed, or its drive disconnected.',
    indexing_walk_ended_volume_missing: 'The scan stopped: the folder read as empty although the index still holds files from it. Nothing was deleted — the drive may not be fully attached.',
    indexing_embed_ended_completed: 'Embedding the whole index has finished.',
    indexing_embed_ended_cancelled: 'The embedding pass was stopped at your request.',
    indexing_embed_ended_failed: 'The embedding pass broke off because something went wrong.',
    indexing_embed_ended_unexpected: 'The embedding pass stopped for a reason not expected here ({reason}).',
    indexing_failure_message: 'The program reported: {message}',
    indexing_walk_result: 'Documents added: {indexed}. Unchanged: {unchanged}. Skipped: {skipped}. Removed from the index: {removed}.',
    indexing_embed_result: 'Chunks embedded: {done} of {total}. Given up on: {refused}.',
    indexing_frozen_heading: 'These subfolders were not reconciled, so files deleted inside them are still found by search:',
    indexing_frozen_row: '{prefix} — {why}',
    indexing_frozen_symlinked_subtree: 'a symbolic link, never entered',
    indexing_frozen_empty_directory: 'read as empty',
    indexing_frozen_unreadable_directory: 'could not be read',
    indexing_note_no_key: 'Search by meaning was not started: no provider key is stored. Word search over this folder already works.',
    indexing_note_no_model: 'Search by meaning was not started: no embedding model has been chosen. Word search over this folder already works.',
    indexing_note_rejected: 'The request was refused.',
    indexing_unobserved: 'Another job is running. This window cannot see how far it has got, but it can still be stopped.',
    indexing_cancel: 'Stop',
  },
};
