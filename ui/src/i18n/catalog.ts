export type Key = 'pin' | 'settings_title' | 'indexed_documents'
  | 'settings_nav_models' | 'settings_nav_folders' | 'settings_nav_scanning' | 'settings_nav_application'
  | 'settings_folders_empty' | 'settings_folders_add'
  | 'settings_folders_load_failed' | 'settings_folders_indexed' | 'settings_folders_remove_named'
  | 'models_provider_label' | 'models_provider_name'
  | 'models_key_label' | 'models_key_saved' | 'models_key_absent_hint'
  | 'models_key_change' | 'models_key_forget' | 'models_key_save' | 'models_key_cancel'
  | 'models_key_removed' | 'models_key_nothing_to_remove'
  | 'models_key_locked' | 'models_key_duplicate' | 'models_key_refused' | 'models_key_defect'
  | 'models_index_not_open' | 'models_index_read_failed'
  | 'models_load_failed'
  | 'models_index_label'
  | 'models_tab_embedding' | 'models_tab_chat'
  | 'models_status_ready' | 'models_status_not_ready'
  | 'models_selection_label' | 'models_selection_not_chosen' | 'models_selection_unknown'
  | 'models_selection_absent'
  | 'models_dot_configured' | 'models_dot_not_configured' | 'models_dot_unknown'
  | 'models_catalogue_empty' | 'models_catalogue_unreadable'
  | 'models_hidden_input_too_small' | 'models_hidden_no_stated_limit'
  | 'models_hidden_limit_not_understood' | 'models_hidden_no_stated_output_modalities'
  | 'models_hidden_no_text_output'
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
  | 'settings_folders_rule_cost_held_below'
  | 'settings_folders_rule_remove' | 'settings_folders_rule_remove_named'
  | 'settings_folders_rule_already_gone' | 'settings_folders_question_withdrawn'
  | 'settings_folders_exclude_checking' | 'settings_folders_include_checking'
  | 'settings_folders_folder_changed' | 'settings_folders_exclude_cost'
  | 'settings_folders_include_cost' | 'settings_folders_include_cost_held_below'
  | 'settings_folders_include_cost_gone'
  | 'settings_folders_confirm_exclude_heading' | 'settings_folders_confirm_include_heading'
  | 'settings_folders_confirm' | 'settings_folders_confirm_cancel'
  | 'settings_folders_confirm_exclude_named' | 'settings_folders_confirm_include_named'
  | 'settings_folders_confirm_cancel_named'
  | 'settings_folders_confirm_remove' | 'settings_folders_confirm_remove_named'
  | 'settings_folders_removing' | 'settings_folders_remove_blocked'
  | 'settings_folders_remove_question_withdrawn'
  | 'settings_folders_added_note'
  | 'settings_masks_heading' | 'settings_masks_explainer' | 'settings_masks_none'
  | 'settings_masks_add' | 'settings_masks_input_label'
  | 'settings_masks_remove' | 'settings_masks_remove_named'
  | 'settings_masks_checking' | 'settings_masks_load_failed'
  | 'settings_masks_confirm_add_heading' | 'settings_masks_confirm_remove_heading'
  | 'settings_masks_add_cost' | 'settings_masks_add_cost_none' | 'settings_masks_remove_cost'
  | 'settings_masks_confirm' | 'settings_masks_confirm_cancel'
  | 'settings_masks_confirm_add_named' | 'settings_masks_confirm_remove_named'
  | 'settings_masks_confirm_cancel_named'
  | 'settings_masks_refused_add' | 'settings_masks_refused_store' | 'settings_masks_refused_remove'
  | 'settings_masks_refused_case_note' | 'settings_masks_already_gone'
  | 'settings_masks_already_stored' | 'settings_masks_question_withdrawn'
  | 'indexing_reading_root'
  | 'indexing_embed_starting_zero' | 'indexing_embed_running' | 'indexing_removing'
  | 'indexing_probe_running' | 'indexing_model_adoption_running' | 'indexing_summary_fallback'
  | 'indexing_counts_ratio' | 'indexing_counts_counting' | 'indexing_counts_contended'
  | 'indexing_eta' | 'indexing_eta_unknown'
  | 'indexing_walk_ended_completed' | 'indexing_walk_ended_partly_read'
  | 'indexing_walk_ended_cancelled' | 'indexing_walk_ended_failed'
  | 'indexing_walk_ended_broken_worker' | 'indexing_walk_ended_rules_not_applied'
  | 'indexing_walk_ended_root_unavailable' | 'indexing_walk_ended_volume_missing'
  | 'indexing_embed_ended_completed' | 'indexing_embed_ended_cancelled'
  | 'indexing_embed_ended_failed' | 'indexing_embed_ended_unexpected'
  | 'indexing_embed_not_started_store'
  | 'indexing_failure_message' | 'indexing_rules_not_applied_pointer'
  | 'indexing_walk_result' | 'indexing_embed_result'
  | 'indexing_frozen_heading' | 'indexing_frozen_row'
  | 'indexing_frozen_symlinked_subtree' | 'indexing_frozen_empty_directory'
  | 'indexing_frozen_unreadable_directory'
  | 'indexing_roots_read' | 'indexing_root_partly_read' | 'indexing_root_unavailable'
  | 'indexing_root_volume_missing' | 'indexing_root_message' | 'indexing_root_cancelled'
  | 'indexing_resume' | 'indexing_retry'
  | 'scanning_scan' | 'scanning_incomplete' | 'scanning_continue_embedding'
  | 'indexing_note_no_key' | 'indexing_note_no_model'
  | 'indexing_cancel'
  | 'indexing_index_files' | 'indexing_index_updated' | 'indexing_index_updated_ago'
  | 'indexing_index_never'
  | 'indexing_index_unreadable_not_open' | 'indexing_index_unreadable_read_failed'
  | 'indexing_index_unreadable_reason' | 'indexing_index_load_failed'
  | 'indexing_index_failed_chunks' | 'indexing_index_refused_run'
  | 'indexing_index_pending_chunks'
  | 'indexing_statcard_documents' | 'indexing_statcard_updated'
  | 'application_group_shortcut' | 'application_group_appearance'
  | 'application_group_startup' | 'application_group_version'
  | 'application_shortcut_label' | 'application_shortcut_registered'
  | 'application_shortcut_unavailable' | 'application_shortcut_reason'
  | 'application_shortcut_tray' | 'application_shortcut_record'
  | 'application_shortcut_recording' | 'application_shortcut_not_usable'
  | 'application_shortcut_failed' | 'application_shortcut_not_saved'
  | 'application_autostart_label' | 'application_autostart_enabled'
  | 'application_autostart_disabled' | 'application_autostart_unknown'
  | 'application_autostart_reason' | 'application_autostart_enable'
  | 'application_autostart_disable' | 'application_autostart_failed'
  | 'application_version' | 'application_load_failed'
  | 'application_theme' | 'theme_light' | 'theme_dark' | 'theme_system'
  | 'application_theme_failed'
  // Task 3 (PR 10f): the language choice, moved out of the tray's temporary
  // submenu into this section. `application_language_uk`/`_en` are endonyms —
  // a language's own name for itself — so they read the SAME in both
  // catalogue blocks below; only `_auto` and every other key here translates.
  // (Review round 1, Minor 6: `locale.rs`'s own `endonym`/`Key::LangAuto`/
  // `Key::MenuLanguage`, which drew the now-deleted tray submenu, are gone —
  // this catalogue's two endonym keys are the only place these strings live
  // now.)
  | 'application_language_label' | 'application_language_auto'
  | 'application_language_uk' | 'application_language_en'
  | 'application_language_partial' | 'application_language_unknown'
  | 'application_language_retry_apply' | 'application_language_retry_read'
  | 'application_language_failed' | 'application_language_change_unconfirmed'
  | 'recent_now' | 'recent_minutes' | 'recent_hours' | 'recent_days';

export const messages: Record<'uk' | 'en', Record<Key, string>> = {
  uk: {
    pin: 'Пін',
    settings_title: 'Налаштування',
    settings_nav_models: 'Моделі',
    settings_nav_folders: 'Теки',
    // Task 8: renamed from «Індексація» — the section renders one «Сканувати»
    // control now, not only what the index holds, and the nav label says what
    // pressing it does.
    settings_nav_scanning: 'Сканування',
    settings_nav_application: 'Застосунок',
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
    // §9.2 review (P2-5): two remove buttons in a two-folder list share one
    // accessible name. `aria-label` carries the folder's own path so a
    // screen reader distinguishes them; the visible button (Task 6: a plain
    // "✕" icon, not this catalogue string) needs no such distinction.
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
    // The lead-in for a rejected read of `model_settings`. The rejection's own
    // sentence is shown verbatim beside it and never branched on (§10): a
    // rejection arrives as text, so this names what failed and the backend says
    // why.
    models_load_failed: 'Не вдалося прочитати налаштування моделей.',
    // Task 5 — the subject header the index sentence lacked: Task 4's review
    // found "Провайдер / [index sentence] / [key sentence]" unreadable as a
    // person, because nothing said the second line was about the index.
    // Shown only alongside that sentence, never on its own.
    models_index_label: 'Індекс:',
    models_tab_embedding: 'Ембединг',
    models_tab_chat: 'Чат',
    // The green-dot rule (§9.1 / the PR 3 ruling `providerReady` already
    // carries): provider + key + a chosen embedding model, fail-safe on
    // anything missing. Named sentences rather than a bare state, so a screen
    // reader announces the same thing a sighted person reads.
    models_status_ready: 'Підключено — OpenRouter, ключ і обрана модель embedding готові.',
    models_status_not_ready: 'Ще не підключено — додайте ключ і оберіть модель embedding, щоб увімкнути пошук за змістом.',
    // Task 4 — the native model select. A label over the control (the same
    // pattern `models_provider_label`/`models_key_label` already use), and
    // the placeholder option's own text for the three states a confirmed
    // model id cannot stand for on its own: a read that said nothing is
    // chosen, a build that currently cannot say either way, and a confirmed
    // id the active catalogue no longer lists.
    models_selection_label: 'Модель:',
    models_selection_not_chosen: 'Модель ще не обрано.',
    models_selection_unknown: 'Поточна модель невідома.',
    models_selection_absent: 'Встановлено «{id}» — постачальник більше не пропонує цю модель.',
    // The per-role configured dot (review P2-1) — its accessible name IS one
    // of these three words; the colour is a visual reinforcement of the same
    // fact, not a second source of it.
    models_dot_configured: 'Налаштовано',
    models_dot_not_configured: 'Не налаштовано',
    models_dot_unknown: 'Невідомо',
    // An empty-but-well-formed catalogue (`models.rs:186-190`) is a stated
    // fact about the provider, not a failure of this build — said once, so a
    // person does not read a blank tab as a bug.
    models_catalogue_empty: 'Постачальник наразі не пропонує жодної моделі для цієї ролі.',
    // `Catalogue.unreadable` (catalogue.rs:246-257): a stated zero is never a
    // promise the list is complete on its own — this sentence is the promise,
    // and it is absent exactly when the count is zero.
    models_catalogue_unreadable: '{count, plural, one {# запис не вдалося прочитати} few {# записи не вдалося прочитати} many {# записів не вдалося прочитати} other {# записів не вдалося прочитати}}.',
    // Owner's ruling (live run, 2026-09-10): `Refusal`'s five variants no
    // longer render inline on a disabled option — the entry disappears from
    // the select outright, and one of these renders below it per DISTINCT
    // reason, with the count of entries it folded together. `{count}` and
    // `{floor}` are always read off the fixture/build data, never a UI
    // literal — see `Models.svelte`'s `hiddenReasonLabel`.
    models_hidden_input_too_small: 'Приховано {count}: ліміт входу менший за {floor} токенів',
    models_hidden_no_stated_limit: 'Приховано {count}: постачальник не вказує ліміт входу',
    models_hidden_limit_not_understood: 'Приховано {count}: ліміт входу у форматі, який ця збірка не читає',
    models_hidden_no_stated_output_modalities: 'Приховано {count}: постачальник не вказує, що видає модель',
    models_hidden_no_text_output: 'Приховано {count}: модель не видає текст',
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
    //
    // «Спершу», а не «щоб змінити»: стан несе НАЙЗОВНІШНІШЕ правило-предка
    // (`tree.rs:755-759`). Якщо правила стоять і на `Archive`, і на
    // `Archive/sub`, прибирання `Archive` не звільняє `Archive/sub/x` — тож
    // речення називає перший крок, а не обіцяє результат.
    //
    // 🔴 Раунд виправлень 2, A1: цей рядок малює ТРИ місця, а не одне — рядок
    // дерева, рядок у переліку правил і питання «більше не виключати». Він тут
    // не з ощадливості: правило під іншим правилом не звільняє нічого, коли
    // його прибрати, і саме це речення вже казало правду про той самий шлях
    // рядком вище. Новий рядок означав би два формулювання одного факту на
    // одному екрані — те, що цей раунд і лагодить. Хто змінює це речення,
    // змінює всі три місця; `Folders.test.ts` читає їх поруч в одному рядку.
    settings_subfolder_excluded_by_ancestor: 'Утримується вашим правилом на {prefix}. Спершу приберіть те правило — теку може утримувати ще одне.',
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
    // 🔴 Рев'ю фінальної гілки, I2. Речення вище безумовне, а стан, який йому
    // суперечить, зберігається без жодної відмови: `exclude_subfolder` не має
    // гарди на предка (`bridge.rs:450-483`), а `add_path_exclusion` —
    // `ON CONFLICT DO NOTHING` (`write.rs:604-612`), тож правила і на
    // `Archive`, і на `Archive/Held` лежать поруч, і `Archive` досі
    // показується як `excluded`. Тоді «все за цим шляхом знову
    // індексуватиметься» спростовує перелік правил двома рядками нижче.
    // Речення не викидають — його роблять правдивим.
    //
    // 🔴 Раунд виправлень 2, A2: «окрім того, що виключають ваші інші правила»
    // — це ОБІЦЯНКА винятку, і правило, чиєї теки на диску немає, її не
    // забезпечує: при наступному скануванні воно не виключає нічого, а та сама
    // панель двома рядками нижче пише про нього `settings_folders_rule_gone`.
    // Тому `heldBelow` читає `existsOnDisk` кожного правила: такий виняток
    // применшив би те, що йде провайдеру, а це єдиний напрям, якого D29 не
    // дозволяє.
    settings_folders_rule_cost_held_below: 'Без цього правила все за цим шляхом знову індексуватиметься від наступного сканування — окрім того, що й далі виключають ваші інші правила глибше за цим шляхом.',
    settings_folders_rule_remove: 'Прибрати правило',
    settings_folders_rule_remove_named: 'Прибрати правило на {prefix}',
    // `include_subfolder` відповідає, чи справді щось прибрали (bridge.rs).
    // «Правила вже не було» — не помилка, а факт про екран, який застарів.
    settings_folders_rule_already_gone: 'Такого правила вже не було. Список перечитано.',
    // PR 8a, Task 8. Числа в питанні заморожені на мить натиску (`Pending`), а
    // сканування — це саме та подія, після якої вони брешуть. Мовчки прибрати
    // питання не можна: натиск людини зник би без сліду. Тому питання знімають
    // і кажуть, про яку теку воно було.
    settings_folders_question_withdrawn: 'Питання про «{path}» знято: індексацію закінчено, і цю панель перечитано. Натисніть ще раз, якщо це досі потрібно.',
    // ── PR 8a, Task 6: що коштує виключення, сказане ДО збереження ──────────
    //
    // Натиснута кнопка перечитує `list_tree` — число зі старого знімка описує
    // мить, яка вже минула. Поки відповідь у дорозі, натиск має бути видимим.
    settings_folders_exclude_checking: 'Перевіряємо, що прибере це виключення…',
    // PR 8a, коло виправлень 5. Натиск «прибрати правило» теж чекає на
    // `list_tree`, але з іншої причини: він нічого не рахує, він з'ясовує, чи це
    // досі та сама тека. Спільне речення на два очікування було б неправдою про
    // одне з них.
    settings_folders_include_checking: 'Перевіряємо, що це досі та сама тека…',
    // 🔴 PR 8a, коло виправлень 5. `watched_root.id` — це `INTEGER PRIMARY KEY`
    // без `AUTOINCREMENT`, тож SQLite видає видалений ідентифікатор наступній
    // теці. Панель могли відкрити для однієї теки, а той самий ідентифікатор
    // тепер належить іншій — і тоді дія пішла б не на ту теку. Нічого не
    // збережено.
    //
    // 🔴 Коло виправлень 6. Раніше речення твердило й «список перечитано» —
    // а `reread` (`Folders.svelte:912-914`) переходить у цей стан ще ДО того,
    // як `refresh()` завершиться, і той виклик може відхилитись: тоді
    // `loadError` показує свій власний рядок з причиною окремим абзацом, а
    // список лишається таким, яким був. Твердження про сам перечит було
    // неправдою саме в тому стані, заради якого банер існує.
    settings_folders_folder_changed: 'Ця тека вже не та, що була на екрані: її місце посіла інша. Нічого не змінено.',
    // 🔴 ДВА числа, і вони про різні речі. `paths` — проіндексовані шляхи під
    // цим префіксом; `documents` — документи, у яких жодного шляху поза ним не
    // лишається. Документ живий, доки його називає бодай один шлях
    // (`forget_if_unnamed`, walk.rs), тож рахувати шляхи й називати їх
    // документами означає завищити втрату. Гілка `=0` — не порожній випадок:
    // це стан, у якому індекс втрачає шлях і не втрачає жодного документа, і
    // сказати про це треба словами, а не нулем.
    // 🔴 Fix round 2, F4. Обидва числа беруться з читання, обмеженого
    // `status = 'indexed'` — того самого обмеження, про яке маска-побратим
    // (`settings_masks_add_cost` нижче) чесно попереджає, а це речення
    // називало своє число без жодного застереження. Додано одну фразу, за
    // зразком маски й у тій самій її частині: наступне сканування може забрати
    // більше, бо файли, які так і не проіндексувалися, тут не враховані.
    // Підмет інший — тека, не маска, — тож формулювання не переписане
    // дослівно: «кожної теки» тут не було б правдою.
    settings_folders_exclude_cost: 'Станом на зараз: при наступному скануванні індекс втратить {paths, plural, one {# файл} few {# файли} many {# файлів} other {# файла}} із цієї теки, а {documents, plural, =0 {жоден документ не перестане знаходитися — кожен із них проіндексовано ще й за іншим шляхом} one {# документ більше не знайдеться: інші шляхи на нього не ведуть} few {# документи більше не знайдуться: інші шляхи на них не ведуть} many {# документів більше не знайдуться: інші шляхи на них не ведуть} other {# документа більше не знайдуться}}. Сканування може прибрати більше: файли, які так і не проіндексувалися, тут не враховані.',
    // Зворотний бік — і свідомо БЕЗ числа: це вікно не знає, що лежить на диску
    // в теці, яку досі оминали, і вигадане там число було б тим самим
    // завищенням, тільки в інший бік. Зате наслідок відомий точно (D29): текст
    // піде провайдеру.
    settings_folders_include_cost: 'Від наступного сканування все всередині цієї теки індексується знову, а її текст надсилається провайдеру моделі.',
    // Той самий стан, що й у `settings_folders_rule_cost_held_below`, тільки в
    // питанні: людину питають про правило, під яким лишаються її ж інші
    // правила. `heldBelow` заморожене в `Pending` разом із `existsOnDisk` — з
    // тієї самої причини: перечитування не має міняти речення під тим, хто
    // його читає. `existsOnDisk` правил нижче — див. `rule_cost_held_below`.
    //
    // 🔴 І цього рядка немає, коли правило тримає ПРЕДОК: тоді питання малює
    // `settings_subfolder_excluded_by_ancestor`, бо прибирання не звільняє
    // нічого, а не «все, окрім глибшого».
    settings_folders_include_cost_held_below: 'Від наступного сканування все всередині цієї теки індексується знову, а її текст надсилається провайдеру моделі — окрім того, що й далі виключають ваші інші правила глибше за цим шляхом.',
    // 🔴 Рев'ю раунду 1 (I1): те саме вікно вже знає, що теки за цим шляхом
    // немає — `existsOnDisk` приходить із бекенда (`bridge.rs:117`) і саме з
    // нього намальовано `settings_folders_rule_gone` у переліку правил тієї ж
    // панелі. Поки питання казало «її текст надсилається провайдеру» над
    // правилом, чия тека зникла, дві фрази однієї панелі суперечили одна одній
    // про ту саму теку.
    // Це не вигадування числа — числа тут як не було, так і немає; це той самий
    // факт, уже прочитаний, сказаний у питанні. Наслідок лишається названим, бо
    // тека може повернутися, а правила вже не буде.
    settings_folders_include_cost_gone: 'Наразі за цим шляхом теки немає, тож зараз нічого не індексується. Якщо тека там з’явиться згодом, вона індексується, а її текст надсилається провайдеру моделі.',
    settings_folders_confirm_exclude_heading: 'Виключити {path}?',
    settings_folders_confirm_include_heading: 'Більше не виключати {path}?',
    settings_folders_confirm: 'Підтвердити',
    settings_folders_confirm_cancel: 'Скасувати',
    // Дві теки на екрані — дві пари кнопок «Підтвердити»/«Скасувати» з тим
    // самим написом; шлях у доступній назві лишає їх розрізненними, як і в
    // `settings_folders_remove_named`.
    settings_folders_confirm_exclude_named: 'Підтвердити виключення {path}',
    settings_folders_confirm_include_named: 'Підтвердити скасування правила на {path}',
    settings_folders_confirm_cancel_named: 'Залишити {path} як є',
    // Task 9. Видалення теки — єдина дія на цьому екрані, яка щось ЗАБИРАЄ з
    // індексу без жодного сканування, тож питання ставиться перед нею так само,
    // як перед виключенням підтеки. Число заморожене на момент натискання
    // (`removeQuestion.files`), а слова — ні: речення будується всередині
    // перебудови під `void $locale`, тож перемикання мови його переписує з тим
    // самим числом.
    //
    // 🔴 Число — це `root.files.length` того рядка, який людина бачила: скільки
    // файлів цієї теки зараз в індексі. Воно не обіцяє нижньої межі й не
    // рахує того, що сканування ще не встигло проіндексувати, — саме тому
    // речення говорить «файли цієї теки», а не «все, що зникне».
    //
    // 🔴 Рев'ю раунду 1 (m1): в арці `other` іменник у родовому однини, тож і
    // дієслово однини — «зникне», як у власній арці `other` ключа
    // `settings_folders_indexed`. Для цілих чисел українське CLDR цієї арки не
    // добирає взагалі, тож на екрані вона не з'являється; вона все одно має
    // бути узгодженою — неузгоджений рядок читається як помилка перекладу
    // всюди, де його побачать.
    settings_folders_confirm_remove: '{files, plural, one {Видалити теку {path} з індексу? # файл цієї теки зникне з пошуку.} few {Видалити теку {path} з індексу? # файли цієї теки зникнуть з пошуку.} many {Видалити теку {path} з індексу? # файлів цієї теки зникнуть з пошуку.} other {Видалити теку {path} з індексу? # файла цієї теки зникне з пошуку.}}',
    // Дві теки на екрані — дві кнопки «Підтвердити» з тим самим написом; шлях у
    // доступній назві лишає їх розрізненними, як у трьох ключах вище.
    settings_folders_confirm_remove_named: 'Підтвердити видалення {path}',
    // Стан рядка, поки `remove_watched_folder` не відповів. Стоїть на місці
    // кнопок цього рядка, а не поруч із ними: натиснути тут більше нема на що,
    // і кнопка, яка нічого не робить, читається як кнопка, яку не почули.
    settings_folders_removing: 'Видаляємо…',
    // Одне речення на весь список, поки триває робота: бекенд відмовляє
    // видалення, доки завдання тримає слот (`bridge.rs:97-180`), тож кнопки
    // вимкнені й тут сказано чому. Не текст відмови — до неї не доходить.
    settings_folders_remove_blocked: 'Кнопка «Видалити» запрацює після зупинки сканування.',
    // 🔴 Рев'ю раунду 1 (m3). Власний ключ, а не `settings_folders_question_withdrawn`.
    // Речення те саме за змістом, але останнє підрядне називає, ЩО перечитано:
    // питання про підтеку живе в панелі, і панель справді перечитують; питання
    // про видалення живе під списком, і рядок, до якого воно належить, у
    // звичайному випадку згорнутий — жодної панелі на екрані немає й жодної не
    // читали. Спільний рядок казав людині про панель, якої вона не бачить.
    settings_folders_remove_question_withdrawn: 'Питання про теку «{path}» знято: індексацію закінчено, і список перечитано. Натисніть ще раз, якщо це досі потрібно.',
    // §9.2, Task 8. Owner's ruling: adding a folder starts no scan — excluding
    // subfolders and setting masks are moves a person may still want to make
    // first — so this sentence stands where the old per-row Scan button's
    // implicit promise used to be, and names the one place a scan now starts.
    settings_folders_added_note: 'Теку додано. Виключіть підтеки й задайте маски, тоді натисніть «Сканувати» у розділі «Сканування».',
    settings_masks_heading: 'Маски файлів',
    // Три факти в одному абзаці, і жоден із них не виводиться з решти екрана:
    // маска глобальна (D-c), тому не стосується тієї теки, поруч з якою вона
    // намальована; кожна тека застосує її на СВОЄМУ наступному скануванні,
    // тому одне сканування нічого не завершує; і регістр не має значення —
    // Task 9b згортає регістр та нормалізує обидві сторони порівняння, тож
    // «*.PDF» і «*.pdf» це одне правило, збережене двома рядками.
    //
    // 🔴 Fix round 2, F6. Речення обіцяло менше, ніж робить правило: воно
    // говорило лише про регістр, а живий прогін зіставив «RÉSUMÉ.TXT» з іменем,
    // записаним на диску В РОЗКЛАДЕНІЙ формі — це нормалізація, а не регістр.
    // Додано одне підрядне про це. Звуження діапазонів ASCII (`[A-z]` → `[a-z]`)
    // сюди свідомо НЕ додано: це запис для леджера, а не для екрана.
    //
    // 🔴 Fix round 4, F2. Останнє підрядне — про «?», і воно тут, а не серед
    // відмов, саме тому, що ця вада є властивістю ІМЕНІ, а не маски: «?.txt»
    // не збігається з «й.txt» через те, що «й» це два байти, і сама маска при
    // цьому суто латинська. Відмовляти на кожен «?» означало б різати по
    // здоровому випадку, тож це сказано словами. Обидва приклади виміряні,
    // не виведені.
    settings_masks_explainer: 'Маска стосується одразу всіх ваших тек: вона порівнюється з іменем файлу на будь-якій глибині. Кожна тека застосує її під час свого наступного сканування. Регістр літер не має значення, тож «*.PDF» і «*.pdf» — це одне й те саме правило; так само не має значення, як саме записані на диску літери з діакритичними знаками. А «?» замінює один байт, а не одну літеру, тож для літер поза латиницею його треба ставити кілька: «?.txt» не збігається з «й.txt», а «??.txt» збігається.',
    settings_masks_none: 'Жодної маски ще не додано.',
    settings_masks_add: 'Додати маску',
    settings_masks_input_label: 'Нова маска:',
    settings_masks_remove: 'Видалити',
    settings_masks_remove_named: 'Видалити маску {mask}',
    settings_masks_checking: 'Перевіряємо, що прибере ця маска…',
    settings_masks_load_failed: 'Не вдалося прочитати список масок.',
    settings_masks_confirm_add_heading: 'Додати маску {mask}?',
    settings_masks_confirm_remove_heading: 'Видалити маску {mask}?',
    // 🔴 Fix round 5, F3, рішення власника: слово «щонайменше» пішло.
    // Воно обіцяло НИЖНЮ межу, а власні коментарі цього ж коду називають два
    // стани, у яких число завищене. `.gitignore` у самій теці не входить у
    // жоден із двох наборів правил (`src-tauri/src/tree.rs`,
    // `crates/mnema-walk/src/rules.rs`), тож файл, який він і так виключає,
    // рахується тут як «уцілілий» і це натискання платить за нього; а набір
    // правил, який не скомпілювався, відповідає ПОРОЖНІМ перекриттям, після
    // чого `walk_root` спиняється перед фазою 2 і не прибирає нічого взагалі —
    // екран сказав би «щонайменше N» про сканування, яке прибирає нуль.
    //
    // 🔴 І це не залишок, успадкований від попереднього речення, а
    // регресія, яку внесла fix round 4. Фраза «під цю маску підпадає
    // щонайменше N файлів» була правдою і при `.gitignore`: виключений файл
    // усе одно підпадає під маску. Нове речення каже про різницю ПРОТИ
    // ПРАВИЛ — а `.gitignore` є одним із правил, які застосовує прохід, — тож
    // його `.gitignore` спростувати може, а попереднє не міг.
    //
    // 🔴 Занижує число теж, і це той самий вимір, що й раніше:
    // `mask_preview.paths` рахує лише рядки зі `status = 'indexed'`
    // (`write.rs`), а множина, яку звіряє прохід, статусу не питає — тож файл
    // документа в стані `pending`, `failed` чи `skipped` прохід прибере, а
    // прев'ю його не показало. Тому остання фраза застерігає в ОБИДВА боки
    // одним реченням: число не межа ні знизу, ні згори.
    //
    // 🔴 Fix round 4, F1: змінилося САМЕ ЧИСЛО, тому мусило змінитися й
    // речення. `mask_preview` більше не рахує «скільки файлів підпадає під цю
    // маску» — воно рахує РІЗНИЦЮ, яку робить це натискання проти всього
    // набору правил, що його застосує наступне сканування. Файл, який уже
    // забирає збережене правило, у число не входить, тож стара фраза «під цю
    // маску підпадає N файлів» стала б меншою за правду.
    //
    // 🔴 Fix round 7, F1, рішення власника: гілка `=0` для документів ПОРОЖНЯ.
    // Три кола поспіль ставили сюди твердження, яке не витримує арифметики, і
    // щоразу воно применшувало втрату. Причина не в словах: `mask_preview`
    // рахує РІЗНИЦЮ між двома наборами правил, а людина питає про світ, і два
    // механізми поза обома наборами забирають у документа останній шлях, який
    // це число називає збереженим — `.gitignore` у самій теці (шлях під ним
    // рахується як «уцілілий») і статус: `Db::indexed_files_under_root`
    // (`crates/mnema-index/src/write.rs`) бере лише `d.status = 'indexed'`, тож
    // єдиний шлях документа в стані `pending`, `failed` чи `skipped` узагалі
    // поза цією множиною. Тому при нулі речення — це число файлів і
    // двобічне застереження, і про документи воно не каже нічого. Кому «і»
    // винесено ВСЕРЕДИНУ ненульових гілок, щоб порожня лишала ціле речення.
    settings_masks_add_cost: 'Станом на зараз ця маска забирає {paths, plural, one {# файл} few {# файли} many {# файлів} other {# файла}} понад те, що вже забирають ваші правила{documents, plural, =0 {} one {, і # документ більше не знайдеться: жодного іншого шляху на нього не залишиться} few {, і # документи більше не знайдуться: жодного іншого шляху на них не залишиться} many {, і # документів більше не знайдуться: жодного іншого шляху на них не залишиться} other {, і # документа більше не знайдуться}}. Наступне сканування кожної теки може прибрати і більше, і менше: файли, які так і не проіндексувалися, тут не враховані, а файл, який уже виключає «.gitignore» у самій теці, тут може бути порахований.',
    // Нуль має власне речення, і після fix round 7 причина цьому одна —
    // ЗАСТЕРЕЖЕННЯ, а не гілка `=0` для документів, якої більше немає. Сусіднє
    // речення хеджує в обидва боки, бо його число буває завищеним; тут число —
    // нуль, і завищеним воно не буває. І нуль тут тепер означає
    // «нічого понад те, що вже забирають ваші правила», а не «під цю маску не
    // підпадає жоден файл»: додати «*.txt» там, де вже збережено «*», дає
    // саме цей нуль, а файли під маску підпадають.
    //
    // Fix round 5, F3: тут «щонайменше» не було й нема чого знімати, і
    // застереження свідомо лишається ОДНОБІЧНИМ. Завищення, через яке сусіднє
    // речення хеджує в обидва боки, може зробити число лише більшим за правду,
    // а тут число — нуль: менше воно не буває. Обидва речення читаються як одне
    // й те саме твердження про той самий залишок, тільки це не обіцяє
    // напрямку, якого не існує.
    settings_masks_add_cost_none: 'Станом на зараз ця маска не забирає нічого понад те, що вже забирають ваші правила. Наступне сканування кожної теки все одно може щось прибрати: файли, які так і не проіндексувалися, тут не враховані.',
    // 🔴 Fix round 4, F3. Речення обіцяло те, чого екран знати не може: з
    // «*.pdf» і «report.*» у сховищі видалення будь-якої з них лишає
    // `report.pdf` виключеним. Полічити це неможливо, і це не обмеження, яке
    // треба обійти — файли, які маска стримувала, В ІНДЕКСІ ВІДСУТНІ, тож
    // жодне читання індексу їх не перелічить (тому на цьому боці й немає
    // прев'ю). Одне речення, безумовно чесне, а не дві гілки за ознакою «чи є
    // ще правило»: «є ще правило» — це не «є правило, що бере ті самі файли»,
    // а другого без імен файлів не знати.
    settings_masks_remove_cost: 'Від наступного сканування кожної теки ця маска більше нічого не стримує: кожен файл, який вона виключала, індексується знову — якщо його не виключає ще якесь ваше правило, — і його текст надсилається провайдеру моделі.',
    settings_masks_confirm: 'Підтвердити',
    settings_masks_confirm_cancel: 'Скасувати',
    settings_masks_confirm_add_named: 'Підтвердити додавання маски {mask}',
    settings_masks_confirm_remove_named: 'Підтвердити видалення маски {mask}',
    settings_masks_confirm_cancel_named: 'Залишити {mask} як є',
    // 🔴 Рамка навколо відмови, а не заміна її. Речення бекенда показується
    // дослівно (жоден компонент не розгалужується на вид помилки), але
    // `reason` всередині нього цитує ЗГОРНУТИЙ взірець: хто ввів «[A-_]x.txt»,
    // читає про «[a-_]x.txt». Тому маску як її ввели називає ця рамка, а
    // наступний рядок пояснює, звідки інший регістр у відповіді.
    settings_masks_refused_add: 'Маску {mask} не додано. Ось що відповіла перевірка:',
    settings_masks_refused_store: 'Маску {mask} не збережено. Ось що відповів індекс:',
    settings_masks_refused_remove: 'Маску {mask} не видалено. Ось що відповів індекс:',
    settings_masks_refused_case_note: 'У цій відповіді маска може бути записана іншим регістром, ніж ви ввели: маски порівнюються без урахування регістру.',
    settings_masks_already_gone: 'Такої маски вже не було. Список перечитано.',
    // 🔴 Близнюк рядка вище, і той самий клас: «нічого не додано» — це окреме
    // речення, а не мовчання. Сховище розрізняє рядки побайтово, а прохід
    // порівнює маски згорнуто, тож «*.PDF» і «*.pdf» — це два можливі рядки й
    // одне правило. Тому тут названо ЗБЕРЕЖЕНЕ написання: людина, якій сказали
    // лише «таке правило вже є», шукала б у переліку те, що щойно ввела, і не
    // знайшла б його.
    settings_masks_already_stored: 'Таке правило у вас уже є — воно записане як «{stored}». Нічого не додано.',
    // 🔴 Власний ключ, а не `settings_folders_question_withdrawn`: там панель
    // ПЕРЕЧИТАНО, а тут застарів РОЗРАХУНОК — питання про маску несе число,
    // яке дав `mask_preview` до того, як індексація змінила набір документів.
    settings_masks_question_withdrawn: 'Питання про маску «{mask}» знято: індексацію закінчено, і розрахунок застарів. Натисніть ще раз, якщо це досі потрібно.',
    // Task 7 — the live reading phase names the folder it is on, one-based the
    // way `scan_job.rs`'s own comment states it ("3 of 7" is what a person
    // reads, and `rootIndex` is the folder being read now, not how many are
    // behind it). No trailing full stop: the sentence ends in an interpolated
    // path, and "…/x." reads as part of the path.
    indexing_reading_root: 'Індексація теки {rootIndex} з {rootCount}: {rootPath}',
    // The embedding pass takes no root and covers the whole index
    // (embed_job.rs), so neither of these two may name the folder that was
    // pressed.
    //
    // Task 7: `indexing_embed_starting` (the old lead-in) is gone. The queue
    // this phase works from is a marker on the index, not a job that "starts"
    // the way a walk does — the ONLY moment a sentence is owed here is a
    // fresh queue reporting zero of zero, which would otherwise read as
    // "nothing to do" while a pass is genuinely under way.
    indexing_embed_starting_zero: 'Вбудовування починається…',
    indexing_embed_running: 'Триває вбудовування всього індексу.',
    // A removal has no counts at all (`scan_state::Phase::Removing`) and is not
    // cancellable (`bridge.rs` fixes it at `false`), so this is the whole of
    // what the strip has to say while it runs.
    indexing_removing: 'Видаляємо теку {rootPath}…',
    // Task 5 — the two jobs nobody asked to start (`scan_state::OtherJob`) get
    // their own name now, so the bottom disclosure has something to say in
    // its summary while either holds the slot, rather than a bare Stop button
    // with no sentence beside it.
    indexing_probe_running: 'Триває перевірка з’єднання…',
    indexing_model_adoption_running: 'Триває заміна моделі вбудовування…',
    // The disclosure's summary always needs a line — see `jobs.ts`'s own
    // priority order — and every real state above already earns one of its
    // own. This is the floor under all of them, never expected to render in
    // practice.
    indexing_summary_fallback: 'Стан індексації',
    indexing_counts_ratio: 'Опрацьовано {done} з {total}. Пропущено: {skipped}. Відхилено: {refused}.',
    // `total: 0` is not an edge case: a walk reports it before phase 1 has
    // counted anything. "0 з 0" would read as "нема чого робити".
    indexing_counts_counting: 'Опрацьовано {done}. Скільки їх усього, поки не відомо. Пропущено: {skipped}. Відхилено: {refused}.',
    // 🔴 Carries NO number of its own, on purpose. `contended` counts files
    // that are also counted in «Пропущено» a moment later, so a number here
    // would read as a second group of files beside that one — and in the one
    // event that arrives before the skip is journalled it would contradict it
    // outright («Пропущено: 0» beside «з них 1»). The line explains part of
    // that number instead.
    //
    // It promises the NEXT scan and nothing else. It must not say the file was
    // recorded: the skip write meets the same lock and can fail too, leaving
    // the file in neither the index nor the journal
    // (`job::Progress::contended`).
    //
    // Task 7: the SAME key answers for `phase.counts.contended` while a scan
    // runs and for `scan.lastReading.contended` once it has ended — the fact
    // it explains ("Пропущено") is the same fact either way, and reading it
    // off `lastReading` is what lets the sentence survive past the ending.
    indexing_counts_contended: 'Індекс саме зайнятий іншим записом, тож частину файлів цей скан не записав. Наступне сканування спробує їх знову.',
    indexing_eta: 'Залишилось приблизно {seconds} с.',
    // `secondsLeft` is `Option<u64>`: "ще не відомо" is a real state, and it is
    // the ordinary one at the start of every run.
    indexing_eta_unknown: 'Скільки ще лишилось часу, поки не відомо.',
    indexing_walk_ended_completed: 'Теки проіндексовано повністю.',
    // `reason: completed` with `complete: false` (job.rs): phase 1 never saw
    // the whole tree, so what stopped being seen is still searchable. That is
    // why the word "done" cannot appear here.
    //
    // 🔴 PR 8a, Task 6 — TWO cases, not one. `should_delete` (walk.rs) keeps
    // every `known` path a frozen prefix covers, and it never asks WHY the path
    // stopped being seen: a file the person deleted and a file their new
    // exclusion rule now covers are the same absence to phase 3. Naming only
    // deletion enumerated what survives and left out the one case PR 8 exists
    // for — the person excludes a folder, the scan says it finished, and the
    // text they meant to withhold is still in the index.
    //
    // 🔴 PR 8a, Task 6 review round 1 (B1) — and TWO axes, not one. The
    // sentence above enumerated WHICH KIND of file survives and then said it
    // happened "усередині них", inside the subfolders that could not be
    // entered. That scope is measurably false: `walk.rs:511` —
    // `if !walked.complete || !stopped_cleanly { return Ok(report); }` — returns
    // before `known` is read, before `frozen` is built and before any
    // `delete_path`, so a partly-read walk reconciles NOTHING under the root.
    // Measured in review round 1, against the real worker and the real
    // database: a rule newly covering a top-level `private/`, a sibling of the
    // unreadable folder and nowhere near it, gave `removed=0`, the path row
    // survived, and the text was still found by search. Under D29 that is the direction that tells a person
    // their text is withheld while it is on its way to the provider.
    //
    // `indexing_frozen_heading` below keeps the narrower scope this sentence
    // gave up: it renders only off a non-empty `frozen`, `report.frozen` is assigned at
    // `walk.rs:747` — past the gate — so `complete: false` always carries
    // `frozen: []`, and on the completed walk that does show it reconciliation
    // ran everywhere except the prefixes it goes on to name. Same class, two
    // different true scopes; each decided on its own evidence.
    indexing_walk_ended_partly_read: 'Теки проіндексовано лише частково: до якихось підтек не вдалося зайти. Нічого в цих теках не звіряли з індексом, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком — не лише всередині тих підтек.',
    indexing_walk_ended_cancelled: 'Сканування зупинено на ваше прохання.',
    indexing_walk_ended_failed: 'Сканування обірвалося через збій.',
    // The four sentences below are not about a malfunction: they are decisions
    // the walk itself made (job.rs), and calling them a failure would tell a
    // person the program broke when instead a folder cannot be read.
    indexing_walk_ended_broken_worker: 'Сканування спинилося: допоміжна програма, яка читає файли, перестала відповідати.',
    indexing_walk_ended_rules_not_applied: 'Сканування спинилося: правила виключення не вдалося застосувати, тож теку не індексували зовсім.',
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
    // `EmbedOutcome::Skipped(StoreUnavailable)` (`scan_state.rs`): the embedding
    // phase was never entered because the credential store itself refused to
    // answer — not "no key", not "no model" — so it earns its own sentence
    // rather than folding into either of those two.
    indexing_embed_not_started_store: 'Вбудовування не запущено: сховище ключів не відповіло: {message}',
    indexing_failure_message: 'Програма повідомила: {message}',
    // The one pointer this catalogue owns for `rulesNotApplied`, shared by
    // both the arm that stops the walk before it starts (this ending never
    // reaches a folder to read) and the arm that stops it mid-walk (the
    // ended report's own `message` names the folder) — a second copy inline
    // in either sentence would be free to drift from this one.
    indexing_rules_not_applied_pointer: 'Виправте правило в розділі «Теки».',
    indexing_walk_result: 'Додано документів: {indexed}. Без змін: {unchanged}. Пропущено: {skipped}. Вилучено з індексу: {removed}.',
    indexing_embed_result: 'Вбудовано фрагментів: {done} з {total}. Відхилено: {refused}.',
    // `frozen` is shown, not dropped: `removed: 0` alone cannot say whether
    // anything was silently left untouched (job.rs).
    // The same two cases as `indexing_walk_ended_partly_read` above, for the
    // same reason, one level more specific: this list NAMES the subtrees.
    //
    // 🔴 And it is the one place the narrower scope IS true — see the B1 note
    // on that key. This heading only ever renders on a walk that reconciled the
    // rest of the root, and only for prefixes that are in fact frozen. It is
    // not, and does not claim to be, a complete account of which exclusions
    // failed to take effect: a rule naming a frozen prefix ITSELF produces no
    // `Frozen` entry at all (measured in review round 1: the walker emits no
    // `NotAFileSubtree` pre-skip for it, the ancestor climb resolves, and the
    // path is deleted normally), so that rule works and is rightly absent from
    // this list.
    indexing_frozen_heading: 'Ці підтеки не звіряли, тож і видалені файли, і файли під вашими правилами виключення досі знаходяться пошуком:',
    indexing_frozen_row: '{prefix} — {why}',
    indexing_frozen_symlinked_subtree: 'символьне посилання, сюди не заходили',
    indexing_frozen_empty_directory: 'прочиталася порожньою',
    indexing_frozen_unreadable_directory: 'не вдалося прочитати',
    // Task 7 — the reading block, drawn from `scan.lastReading` in `idle` and
    // `ended` alike (D-e), and outliving the report that ends beside it.
    // No trailing full stop on `indexing_roots_read`: it sits ahead of the
    // result sentence on its own line, not as that sentence's own clause.
    indexing_roots_read: 'Проіндексовано тек: {rootsRead} з {rootCount}',
    // One row per root whose reading did not simply complete
    // (`readingKind(root) !== 'completed'`) — each names its own path, because
    // the aggregate cannot say WHICH folder the fact is about.
    indexing_root_partly_read: '{rootPath}: проіндексовано частково',
    indexing_root_unavailable: '{rootPath}: тека недоступна',
    indexing_root_volume_missing: '{rootPath}: том відсутній',
    // F6 (Task 10 live run): a cancelled root used to fall through to
    // `indexing_root_message`'s own fallback, `WALK_ENDED['cancelled']` — the
    // very sentence `readingBlock.sentence` already draws once for the whole
    // reading. A root row and the reading's own outcome sentence saying the
    // same thing twice read as an error in this component, not as agreement.
    indexing_root_cancelled: '{rootPath}: індексацію перервано',
    // `failed`/`brokenWorker` at the root — `job.rs`'s `message` is the one
    // thing that tells a broken pool, a missing worker binary and a panic
    // apart, per root the same way `indexing_failure_message` does for the
    // whole scan.
    indexing_root_message: '{rootPath}: {message}',
    // D-m's labels: a person who pressed Stop is RESUMING, one whose scan
    // failed is RETRYING — one word for both would read a failure as their
    // own doing.
    indexing_resume: 'Продовжити',
    indexing_retry: 'Повторити',
    // §9.3, Task 8 — the Scanning section's own ONE control, shown whenever no
    // run already owns the slot.
    scanning_scan: 'Сканувати',
    // The marker `continueAction` (`jobs.ts`) reads FIRST, ahead of the queue:
    // a half-read archive says so before it says anything about what is
    // waiting to be embedded, because embedding what is there and leaving the
    // unread half invisible would be the worse silence of the two.
    scanning_incomplete: 'Попереднє сканування не завершило індексацію тек.',
    // F2 (Task 10 live run): the QUEUE arm of `continueAction`
    // (`entry: 'embedOnly'`, `where: 'section'`) used to share `indexing_resume`
    // with the marker arm (`entry: 'full'`), so a person offered to resume the
    // embedding queue alone read a button that said only «Продовжити» — the
    // same word a half-read archive's own resume button says, promising the
    // wrong half of the work. This key is that arm's own, and it alone: the
    // marker arm still keeps `indexing_resume`.
    scanning_continue_embedding: 'Продовжити вбудовування',
    // F11 (Task 10 live run, owner's ruling): «у цій теці» claimed a scope this
    // sentence never had — word search covers every watched folder, not the
    // one this note happens to be drawn beside — so the clause is dropped
    // rather than corrected to name all of them.
    indexing_note_no_key: 'Пошук за змістом не вмикали: ключ провайдера не збережено. Пошук по словах уже працює.',
    indexing_note_no_model: 'Пошук за змістом не вмикали: модель вбудовування не обрана. Пошук по словах уже працює.',
    indexing_cancel: 'Зупинити',
    // §9.3, PR 9 Task 6 — the Scanning SECTION (called Indexing before Task
    // 8), which says what the index holds. Every key here is
    // `indexing_index_*` so nothing confuses it with the `indexing_*` keys
    // above, which belong to the window's job strip and
    // say what a pass is doing right now.
    //
    // The count is of `path` rows, not of documents (D-e): a file in two
    // watched folders counts twice, which is what makes this number the sum of
    // the per-folder numbers the Folders rows draw beside it. The wording says
    // «файл», the same word those rows' subject implies, rather than
    // «документ» — the tidier definition is the one that would disagree with
    // the screen next to it.
    indexing_index_files: '{count, plural, one {В індексі # файл} few {В індексі # файли} many {В індексі # файлів} other {В індексі # файла}}.',
    // The date and the relative phrase are two lines, not one sentence: the
    // date is what a person compares against the file they edited this
    // morning, the phrase is what they feel. `formatIndexedDate` fills {date}.
    indexing_index_updated: 'Останнє оновлення: {date}.',
    // The phrase gets a subject and a full stop of its own. `formatIndexedAt`
    // returns «1 годину тому», which the Recents card can render bare because a
    // filename sits beside it supplying the subject; last on this panel, with
    // nothing beside it, it read as an orphan fragment (review, Minor 4).
    indexing_index_updated_ago: 'Це було {ago}.',
    // `lastIndexedAt: null` is the backend's own statement that nothing has
    // ever finished indexing. Never a blank, never an epoch date.
    indexing_index_never: 'Ще нічого не проіндексовано.',
    // Task 6: the statcard's two cells — the same numbers the sentences above
    // already state, drawn beside a short label instead of inside one. Not a
    // decorative number: `indexing_statcard_documents`'s value is
    // `read.indexedFiles`, and `indexing_statcard_updated`'s is the existing
    // `formatIndexedDate`/`neverLine`, never a value invented for the card.
    indexing_statcard_documents: 'Документів',
    indexing_statcard_updated: 'Останнє оновлення',
    // Two causes, two sentences (`UnreadableCause`, models.rs:809-826). One
    // sentence for both would be a surface that cannot tell a closed index
    // from one that broke while being read.
    indexing_index_unreadable_not_open: 'Не вдалося прочитати індекс: він не відкритий.',
    indexing_index_unreadable_read_failed: 'Не вдалося прочитати індекс: спроба читання не вдалася.',
    // 🔴 The backend's `reason` goes here VERBATIM, and that is deliberate —
    // the opposite of the Models section's rule. `IndexSettings::Unreadable`'s
    // own doc says `reason` "stays verbatim, for showing" (models.rs:932); this
    // is the one screen whose job is to tell a person what is wrong with their
    // index, the text never leaves the machine (D22), and the path inside it is
    // the actionable part. `cause` above is what anything BRANCHES on.
    indexing_index_unreadable_reason: 'Програма повідомила: {reason}',
    // A rejected `model_settings` — §10: the backend's sentence is shown
    // verbatim beside this lead-in, never branched on, and no numbers are drawn
    // from a read that failed.
    indexing_index_load_failed: 'Не вдалося прочитати стан індексу.',
    // 🔴 The two scopes, owed since PR 7 (`job::Progress::refused` is THIS
    // RUN's, `IndexRead::failed_chunks` is the SPACE's — job.rs:38-44 says so
    // in as many words). Two sentences, each naming its own subject, because a
    // person seeing one number under two meanings cannot tell which they got.
    // This one is the space, and it states the rule the count exists to make
    // defensible: the chunk has left the embedding queue for good until its
    // text changes, so search by meaning stops answering for it while the
    // document still shows it and word search still finds it.
    indexing_index_failed_chunks: 'У цьому індексі провайдер відхилив {count, plural, one {# фрагмент} few {# фрагменти} many {# фрагментів} other {# фрагмента}} за весь час. Їх більше не пропонують, доки не зміниться їхній текст: пошук за змістом їх не знаходить, пошук по словах — знаходить.',
    // And this one is the run that has just ended. Its subject is a pass, not
    // the index.
    indexing_index_refused_run: 'Останній прохід вбудовування відхилив {count, plural, one {# фрагмент} few {# фрагменти} many {# фрагментів} other {# фрагмента}}.',
    // F4 (spec §9.3, amended 2026-09-04): the embedding queue, `IndexRead.
    // pendingChunks` — a tray Stop mid-pass, then a restart, left thousands of
    // chunks un-embedded with nothing on screen saying so. Task 8: the button
    // beside it is `indexing_resume` now, through `continueAction`'s
    // `where: 'section'`/`entry: 'embedOnly'` offer, not a button of its own —
    // `indexing_index_resume_embedding` is gone, grepped for other consumers
    // first (none found).
    indexing_index_pending_chunks: '{count, plural, one {Ще не вбудовано # фрагмент} few {Ще не вбудовано # фрагменти} many {Ще не вбудовано # фрагментів} other {Ще не вбудовано # фрагмента}}.',
    // §9.4 — the Application section: the shortcut, autostart, and the version.
    //
    // 🔴 Two sentence sources, and they are kept apart on purpose. Everything
    // below is a refusal or a statement the WINDOW makes, so it lives here in
    // both languages. A refusal the BACKEND makes is shown verbatim beside
    // these and is English, like every other rejection in this product.
    // Task 6: the four group headings the mockup arranges this section into
    // (mockup .gh, `.spane h3`'s own styling). The controls under each are
    // unchanged; only the labelled `role="group"` wrapper around them is new.
    application_group_shortcut: 'Виклик',
    application_group_appearance: 'Вигляд',
    application_group_startup: 'Запуск',
    application_group_version: 'Версія',
    application_shortcut_label: 'Скорочення для відкриття пошуку:',
    // 🔴 «Зареєстровано» — і ніколи «працює» чи «належить лише вам». D128
    // виміряв, що macOS реєструє скорочення, яке вже тримає інший застосунок:
    // реєструються обидва і спрацьовують обидва. Речення не має права
    // обіцяти більше, ніж повідомляє операційна система.
    application_shortcut_registered: 'Це скорочення зареєстровано в системі.',
    application_shortcut_unavailable: 'Це скорочення не зареєстровано в системі.',
    application_shortcut_reason: 'Програма повідомила: {reason}',
    // Стан, який нічого не пропонує далі, — це стан, про який пишуть у
    // підтримку. Пошук залишається досяжним, і секція каже як саме.
    application_shortcut_tray: 'Пошук усе одно можна відкрити з піктограми застосунку в системному лотку.',
    application_shortcut_record: 'Змінити скорочення',
    application_shortcut_recording: 'Натисніть потрібне сполучення клавіш. Escape залишає скорочення без змін.',
    // {mod}: the platform's own name for the fourth modifier — Cmd/Win/Super
    // (`shortcut.ts`'s `MODIFIER_KEY_NAME`) — never the platform-neutral
    // "командною", which named the wrong key on Windows and Linux (review,
    // Minor 5).
    application_shortcut_not_usable: 'Цю клавішу не можна використати в скороченні. Скорочення — це літера, цифра, функційна клавіша, стрілка або пробіл, натиснуті разом принаймні з однією з клавіш Ctrl, Alt, Shift чи {mod}.',
    application_shortcut_failed: 'Скорочення не змінено. Ось що відповів застосунок:',
    // 🔴 Зовнішнє рев'ю P3. Рядок 6 таблиці переходів (`prefs.rs`): операційна
    // система вже зареєструвала НОВЕ скорочення, а запис у `prefs.json` не
    // вдався. Скорочення діє просто зараз і зникне після перезапуску — тож
    // «не змінено» поруч із ним є неправдою в обидва боки. Обирається за
    // ПЕРЕЧИТАНИМ станом, ніколи за розбором речення відмови.
    application_shortcut_not_saved: 'Скорочення діє, але зберегти його не вдалося: після перезапуску повернеться попереднє. Ось що відповів застосунок:',
    application_autostart_label: 'Запуск під час входу в систему:',
    application_autostart_enabled: 'Mnema запускається під час входу в систему.',
    application_autostart_disabled: 'Mnema не запускається під час входу в систему.',
    // 🔴 Третє речення, а не друге вдруге: невдале читання, показане як «не
    // запускається», показало б людині перемикач у положенні, протилежному до
    // того, у якому насправді перебуває машина.
    application_autostart_unknown: 'Не вдалося дізнатися, чи запускається Mnema під час входу в систему.',
    application_autostart_reason: 'Програма повідомила: {reason}',
    application_autostart_enable: 'Запускати під час входу',
    application_autostart_disable: 'Не запускати під час входу',
    application_autostart_failed: 'Налаштування не змінено. Ось що відповів застосунок:',
    // D-h: версію показано як є, включно з 0.0.0. Поруч немає «у вас найновіша
    // версія» — цього ніхто не перевіряв.
    application_version: 'Версія {version}',
    application_load_failed: 'Не вдалося прочитати налаштування застосунку.',
    application_theme: 'Тема:',
    theme_light: 'Світла',
    theme_dark: 'Темна',
    theme_system: 'Системна',
    application_theme_failed: 'Вигляд не змінено. Ось що відповів застосунок:',
    // Task 3 (PR 10f). `_uk`/`_en` are endonyms — a language's own name for
    // itself — and read the same in the `en` block below. `locale.rs`'s own
    // `endonym`, which gave these same two strings to the tray submenu this
    // replaces, is deleted (review round 1, Minor 6): this catalogue is the
    // only place they live now.
    application_language_label: 'Мова:',
    application_language_auto: 'Авто (система)',
    application_language_uk: 'Українська',
    application_language_en: 'English',
    // Exact string, pinned by `Application.test.ts` against the brief.
    application_language_partial: 'Мову збережено, але застосовано не всюди',
    application_language_unknown: 'Застосування мови не підтверджено.',
    application_language_retry_apply: 'Повторити застосування',
    application_language_retry_read: 'Повторити читання',
    application_language_failed: 'Не вдалося прочитати мову. Ось що відповів застосунок:',
    // Distinct from `_failed` above: that one is a failed READ, this one a
    // rejected CHANGE — two different operations, shown under two different
    // headings (review round 1, Minor 3), the same way `application_shortcut_
    // failed`/`application_autostart_failed`/`application_theme_failed` each
    // name their own control rather than sharing one sentence.
    //
    // NEVER "не змінено" / "was not changed" (review round 2, Important A):
    // `application.kind === 'unknown'` means persist-vs-transport could not
    // be told apart from the message alone — the change may well have
    // applied and only its REPLY got lost. "Не підтверджено" states only
    // what is actually known.
    application_language_change_unconfirmed: 'Зміну мови не підтверджено. Ось що відповів застосунок:',
  },
  en: {
    pin: 'Pin',
    settings_title: 'Settings',
    settings_nav_models: 'Models',
    settings_nav_folders: 'Folders',
    settings_nav_scanning: 'Scanning',
    settings_nav_application: 'Application',
    settings_folders_empty: 'No folder has been added yet.',
    settings_folders_add: 'Add a folder',
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
    models_load_failed: 'The model settings could not be read.',
    models_index_label: 'Index:',
    models_tab_embedding: 'Embedding',
    models_tab_chat: 'Chat',
    models_status_ready: 'Connected — OpenRouter, a key and a chosen embedding model are all set.',
    models_status_not_ready: 'Not connected yet — add a key and choose an embedding model to enable content search.',
    models_selection_label: 'Model:',
    models_selection_not_chosen: 'No model chosen yet.',
    models_selection_unknown: 'The current model is unknown.',
    models_selection_absent: 'Set to "{id}", which the provider no longer lists.',
    models_dot_configured: 'Configured',
    models_dot_not_configured: 'Not configured',
    models_dot_unknown: 'Unknown',
    models_catalogue_empty: 'The provider does not currently list any models for this role.',
    models_catalogue_unreadable: '{count, plural, one {# record could not be read} other {# records could not be read}}.',
    models_hidden_input_too_small: '{count} hidden: input limit below {floor} tokens',
    models_hidden_no_stated_limit: '{count} hidden: the provider states no input limit',
    models_hidden_limit_not_understood: '{count} hidden: input limit in a format this build cannot read',
    models_hidden_no_stated_output_modalities: '{count} hidden: the provider does not say what the model outputs',
    models_hidden_no_text_output: '{count} hidden: the model outputs no text',
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
    settings_folders_expand: 'Subfolders',
    settings_folders_expand_named: 'Subfolders of {path}',
    settings_subfolders_loading: 'Reading the subfolders…',
    settings_subfolders_none: 'This folder has no subfolders.',
    settings_subfolders_unnameable: '{count, plural, one {# subfolder is not listed: its name could not be read as text.} other {# subfolders are not listed: their names could not be read as text.}}',
    settings_subfolders_failed: 'The subfolders of this folder could not be read.',
    settings_subfolder_open: 'No rule excludes this folder.',
    settings_subfolder_excluded: 'Excluded by your rule.',
    settings_subfolder_excluded_by_ancestor: 'Held by your rule on {prefix}. Remove that rule first — another rule may still hold this folder.',
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
    settings_folders_rule_cost_held_below: 'Without this rule, anything at this path is indexed again from the next scan on — except what your other rules further down this path still exclude.',
    settings_folders_rule_remove: 'Remove the rule',
    settings_folders_rule_remove_named: 'Remove the rule on {prefix}',
    settings_folders_rule_already_gone: 'There was no such rule left to remove. The list has been re-read.',
    settings_folders_question_withdrawn: 'The question about “{path}” has been withdrawn: indexing has finished and this panel was read again. Press again if you still want to.',
    settings_folders_exclude_checking: 'Checking what this exclusion removes…',
    settings_folders_include_checking: 'Checking that this is still the same folder…',
    // 🔴 Fix round 6. Used to also claim "and the list has been re-read" —
    // `reread` (`Folders.svelte:912-914`) reaches this state before `refresh()`
    // settles, and that call can be rejected: `loadError` then prints its own
    // sentence in a separate paragraph while the list stays exactly as stale as
    // it was. The re-read claim was false in the one state this banner exists
    // for.
    settings_folders_folder_changed: 'This folder is no longer the one that was on screen: another folder has taken its place. Nothing was changed.',
    settings_folders_exclude_cost: 'As of now: on the next scan the index loses {paths, plural, one {# file} other {# files}} from this folder, and {documents, plural, =0 {no document stops being findable — each is also indexed under another path} one {# document stops being findable: no other path names it} other {# documents stop being findable: no other path names them}}. The scan can remove more than that: files that never finished indexing are not counted here.',
    settings_folders_include_cost: 'From the next scan on, everything inside this folder is indexed again, and its text is sent to the model provider.',
    settings_folders_include_cost_held_below: 'From the next scan on, everything inside this folder is indexed again, and its text is sent to the model provider — except what your other rules further down this path still exclude.',
    settings_folders_include_cost_gone: 'There is no folder at this path right now, so nothing is being indexed today. If a folder appears there later, it is indexed and its text is sent to the model provider.',
    settings_folders_confirm_exclude_heading: 'Exclude {path}?',
    settings_folders_confirm_include_heading: 'Stop excluding {path}?',
    settings_folders_confirm: 'Confirm',
    settings_folders_confirm_cancel: 'Cancel',
    settings_folders_confirm_exclude_named: 'Confirm excluding {path}',
    settings_folders_confirm_include_named: 'Confirm not excluding {path}',
    settings_folders_confirm_cancel_named: 'Leave {path} as it is',
    settings_folders_confirm_remove: '{files, plural, one {Remove folder {path} from the index? # file from this folder will disappear from search.} other {Remove folder {path} from the index? # files from this folder will disappear from search.}}',
    settings_folders_confirm_remove_named: 'Confirm removing {path}',
    settings_folders_removing: 'Removing…',
    settings_folders_remove_blocked: 'The Remove button works again once the scan is stopped.',
    settings_folders_remove_question_withdrawn: 'The question about folder “{path}” has been withdrawn: indexing has finished and the list was read again. Press again if you still want to.',
    settings_folders_added_note: 'Folder added. Exclude subfolders and set masks, then press “Scan” in the Scanning section.',
    settings_masks_heading: 'File masks',
    settings_masks_explainer: 'A mask applies to every watched folder at once: it is compared with a file name, at any depth. Each folder applies it on its own next scan. Letter case does not matter, so *.PDF and *.pdf are one and the same rule; neither does the way a name happens to store its accents. And ? stands for a single byte rather than a single letter, so a letter outside the basic Latin alphabet needs more than one of them: ?.txt does not match й.txt, and ??.txt does.',
    settings_masks_none: 'No file mask has been added yet.',
    settings_masks_add: 'Add a mask',
    settings_masks_input_label: 'New mask:',
    settings_masks_remove: 'Remove',
    settings_masks_remove_named: 'Remove the mask {mask}',
    settings_masks_checking: 'Checking what this mask removes…',
    settings_masks_load_failed: 'The list of masks could not be read.',
    settings_masks_confirm_add_heading: 'Add the mask {mask}?',
    settings_masks_confirm_remove_heading: 'Remove the mask {mask}?',
    settings_masks_add_cost: 'As of now this mask takes {paths, plural, one {# file} other {# files}} beyond what your rules already take{documents, plural, =0 {} one {, and # document stops being findable: no other path will be left naming it} other {, and # documents stop being findable: no other path will be left naming them}}. The next scan of each folder can remove more than that or fewer: files that never finished indexing are not counted here, and a file a .gitignore in the folder itself already excludes may be counted here.',
    settings_masks_add_cost_none: 'As of now this mask takes nothing beyond what your rules already take. The next scan of each folder can still remove files: those that never finished indexing are not counted here.',
    settings_masks_remove_cost: 'From the next scan of each folder on, this mask stops holding anything back: each file it was excluding is indexed again — unless another of your rules still excludes it — and its text is sent to the model provider.',
    settings_masks_confirm: 'Confirm',
    settings_masks_confirm_cancel: 'Cancel',
    settings_masks_confirm_add_named: 'Confirm adding the mask {mask}',
    settings_masks_confirm_remove_named: 'Confirm removing the mask {mask}',
    settings_masks_confirm_cancel_named: 'Leave {mask} as it is',
    settings_masks_refused_add: 'The mask {mask} was not added. This is what the check answered:',
    settings_masks_refused_store: 'The mask {mask} was not stored. This is what the index answered:',
    settings_masks_refused_remove: 'The mask {mask} was not removed. This is what the index answered:',
    settings_masks_refused_case_note: 'The answer above can quote your mask in a different letter case than the one you typed: masks are compared with letter case ignored.',
    settings_masks_already_gone: 'There was no such mask left to remove. The list has been re-read.',
    settings_masks_already_stored: 'You already have this rule — it is stored as {stored}. Nothing was added.',
    settings_masks_question_withdrawn: 'The question about mask “{mask}” has been withdrawn: indexing has finished and the estimate is stale. Press again if you still want to.',
    indexing_reading_root: 'Indexing folder {rootIndex} of {rootCount}: {rootPath}',
    indexing_embed_starting_zero: 'Embedding is starting…',
    indexing_embed_running: 'The whole index is being embedded.',
    indexing_removing: 'Removing the folder {rootPath}…',
    indexing_probe_running: 'Checking the connection…',
    indexing_model_adoption_running: 'Switching the embedding model…',
    indexing_summary_fallback: 'Indexing status',
    indexing_counts_ratio: 'Processed {done} of {total}. Skipped: {skipped}. Given up on: {refused}.',
    indexing_counts_counting: 'Processed {done}. How many there are in total is not known yet. Skipped: {skipped}. Given up on: {refused}.',
    indexing_counts_contended: 'The index is busy with another write, so this scan did not write some files. The next scan will try them again.',
    indexing_eta: 'About {seconds} s left.',
    indexing_eta_unknown: 'How much time is left is not known yet.',
    indexing_walk_ended_completed: 'The folders were indexed in full.',
    indexing_walk_ended_partly_read: 'The folders were only partly indexed: some subfolders could not be entered. Nothing in these folders was checked against the index, so both deleted files and files your exclusion rules now cover are still found by search — not only inside those subfolders.',
    indexing_walk_ended_cancelled: 'The scan was stopped at your request.',
    indexing_walk_ended_failed: 'The scan broke off because something went wrong.',
    indexing_walk_ended_broken_worker: 'The scan stopped: the helper program that reads files stopped answering.',
    indexing_walk_ended_rules_not_applied: 'The scan stopped: the exclusion rules could not be applied, so the folder was not indexed at all.',
    indexing_walk_ended_root_unavailable: 'The scan stopped: the folder could not be entered. It may have been removed, or its drive disconnected.',
    indexing_walk_ended_volume_missing: 'The scan stopped: the folder read as empty although the index still holds files from it. Nothing was deleted — the drive may not be fully attached.',
    indexing_embed_ended_completed: 'Embedding the whole index has finished.',
    indexing_embed_ended_cancelled: 'The embedding pass was stopped at your request.',
    indexing_embed_ended_failed: 'The embedding pass broke off because something went wrong.',
    indexing_embed_ended_unexpected: 'The embedding pass stopped for a reason not expected here ({reason}).',
    indexing_embed_not_started_store: 'Embedding was not started: the key store did not answer: {message}',
    indexing_failure_message: 'The program reported: {message}',
    indexing_rules_not_applied_pointer: 'Fix the rule in the Folders section.',
    indexing_walk_result: 'Documents added: {indexed}. Unchanged: {unchanged}. Skipped: {skipped}. Removed from the index: {removed}.',
    indexing_embed_result: 'Chunks embedded: {done} of {total}. Given up on: {refused}.',
    indexing_frozen_heading: 'These subfolders were not reconciled, so both deleted files and files your exclusion rules now cover are still found by search inside them:',
    indexing_frozen_row: '{prefix} — {why}',
    indexing_frozen_symlinked_subtree: 'a symbolic link, never entered',
    indexing_frozen_empty_directory: 'read as empty',
    indexing_frozen_unreadable_directory: 'could not be read',
    indexing_roots_read: 'Folders indexed: {rootsRead} of {rootCount}',
    indexing_root_partly_read: '{rootPath}: indexed partly',
    indexing_root_unavailable: '{rootPath}: the folder is unavailable',
    indexing_root_volume_missing: '{rootPath}: the volume is missing',
    indexing_root_cancelled: '{rootPath}: indexing was interrupted',
    indexing_root_message: '{rootPath}: {message}',
    indexing_resume: 'Resume',
    indexing_retry: 'Retry',
    scanning_scan: 'Scan',
    scanning_incomplete: 'The previous scan did not finish indexing the folders.',
    scanning_continue_embedding: 'Continue embedding',
    indexing_note_no_key: 'Search by meaning was not started: no provider key is stored. Word search already works.',
    indexing_note_no_model: 'Search by meaning was not started: no embedding model has been chosen. Word search already works.',
    indexing_cancel: 'Stop',
    indexing_index_files: '{count, plural, one {The index holds # file} other {The index holds # files}}.',
    indexing_index_updated: 'Last updated: {date}.',
    indexing_index_updated_ago: 'That was {ago}.',
    indexing_index_never: 'Nothing has been indexed yet.',
    indexing_statcard_documents: 'Documents',
    indexing_statcard_updated: 'Last update',
    indexing_index_unreadable_not_open: 'The index could not be read: it is not open.',
    indexing_index_unreadable_read_failed: 'The index could not be read: the attempt to read it failed.',
    indexing_index_unreadable_reason: 'The program reported: {reason}',
    indexing_index_load_failed: 'The state of the index could not be read.',
    indexing_index_failed_chunks: 'In this index the provider has given up on {count, plural, one {# chunk} other {# chunks}} in all. They are not offered again until their text changes: search by meaning does not find them, word search still does.',
    indexing_index_refused_run: 'The last embedding pass gave up on {count, plural, one {# chunk} other {# chunks}}.',
    indexing_index_pending_chunks: '{count, plural, one {# chunk is not embedded yet} other {# chunks are not embedded yet}}.',
    // §9.4 — the Application section: the shortcut, autostart, and the version.
    //
    // 🔴 Two sentence sources, and they are kept apart on purpose. Everything
    // below is a refusal or a statement the WINDOW makes, so it lives here in
    // both languages. A refusal the BACKEND makes is shown verbatim beside
    // these and is English, like every other rejection in this product.
    application_group_shortcut: 'Shortcut',
    application_group_appearance: 'Appearance',
    application_group_startup: 'Startup',
    application_group_version: 'Version',
    application_shortcut_label: 'Shortcut for opening the search:',
    application_shortcut_registered: 'This shortcut is registered with the system.',
    application_shortcut_unavailable: 'This shortcut is not registered with the system.',
    application_shortcut_reason: 'The program reported: {reason}',
    application_shortcut_tray: 'The search can still be opened from the application icon in the tray.',
    application_shortcut_record: 'Change the shortcut',
    application_shortcut_recording: 'Press the combination you want. Escape leaves the shortcut as it is.',
    application_shortcut_not_usable: 'That key cannot be used in a shortcut. A shortcut is a letter, a digit, a function key, an arrow or the space bar, held together with at least one of Ctrl, Alt, Shift or {mod}.',
    application_shortcut_failed: 'The shortcut was not changed. This is what the application answered:',
    application_shortcut_not_saved: 'The shortcut is in effect, but it could not be saved: the previous one returns after a restart. This is what the application answered:',
    application_autostart_label: 'Starting when you sign in:',
    application_autostart_enabled: 'Mnema starts when you sign in.',
    application_autostart_disabled: 'Mnema does not start when you sign in.',
    application_autostart_unknown: 'Whether Mnema starts when you sign in could not be read.',
    application_autostart_reason: 'The program reported: {reason}',
    application_autostart_enable: 'Start when I sign in',
    application_autostart_disable: 'Do not start when I sign in',
    application_autostart_failed: 'The setting was not changed. This is what the application answered:',
    application_version: 'Version {version}',
    application_load_failed: 'The application settings could not be read.',
    application_theme: 'Theme:',
    theme_light: 'Light',
    theme_dark: 'Dark',
    theme_system: 'Match the system',
    application_theme_failed: 'The appearance was not changed. This is what the application answered:',
    application_language_label: 'Language:',
    application_language_auto: 'Auto (system)',
    application_language_uk: 'Українська',
    application_language_en: 'English',
    // Exact string, pinned by `Application.test.ts` against the brief.
    application_language_partial: 'Language saved, but not applied everywhere',
    application_language_unknown: 'Whether the language applied everywhere could not be confirmed.',
    application_language_retry_apply: 'Retry applying',
    application_language_retry_read: 'Retry reading',
    application_language_failed: 'The language could not be read. This is what the application answered:',
    application_language_change_unconfirmed: 'The language change could not be confirmed. This is what the application answered:',
  },
};
