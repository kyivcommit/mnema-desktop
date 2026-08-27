export type Key = 'pin' | 'settings_title' | 'indexed_documents'
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
  | 'citations_only_banner' | 'citations_only_banner_empty' | 'citations_only_empty';

export const messages: Record<'uk' | 'en', Record<Key, string>> = {
  uk: {
    pin: 'Пін',
    settings_title: 'Налаштування',
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
    citations_only_banner: 'Генерування недоступне. Нижче — уривки, які знайшов пошук.',
    citations_only_banner_empty: 'Генерування недоступне.',
    // Ruling AK: its own sentence, distinct from `tree_empty` (nothing indexed
    // at all) and from `source_failed` (a passage that could not be read).
    citations_only_empty: 'Жоден уривок не відповідає цьому запиту.',
  },
  en: {
    pin: 'Pin',
    settings_title: 'Settings',
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
    citations_only_banner: 'Generation is unavailable. These are the passages the search found.',
    citations_only_banner_empty: 'Generation is unavailable.',
    citations_only_empty: 'No passages matched this query.',
  },
};
