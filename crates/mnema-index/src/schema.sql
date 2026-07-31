-- G7.0 §5. One database per installation. Every table here is plain SQLite;
-- vector tables are created lazily at runtime and are NEVER altered or renamed.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- ---------------------------------------------------------------- catalogue

CREATE TABLE watched_root (
    id            INTEGER PRIMARY KEY,
    absolute_path TEXT NOT NULL UNIQUE,   -- local setting; never travels with the file
    added_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE tag (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE             -- renaming costs one row
);

-- A live rule: everything under this prefix carries this tag, recursively.
CREATE TABLE tag_rule (
    id              INTEGER PRIMARY KEY,
    tag_id          INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    watched_root_id INTEGER NOT NULL REFERENCES watched_root(id) ON DELETE CASCADE,
    path_prefix     TEXT NOT NULL,
    UNIQUE(tag_id, watched_root_id, path_prefix)
);

-- provenance: 'rule' rows are recomputed by reconciliation; 'manual' and
-- 'removed' are never touched by it. Without 'removed' the scan re-applies a
-- tag the user has just taken off. G7.0 §1.1.
--
-- The three provenances are exclusive states of one (document, tag) pair, so
-- provenance is a plain column and stays OUT of the primary key. Inside it, a
-- pair could be 'manual' and 'removed' at once and "is this tag on?" would have
-- no answer from the rows alone.
CREATE TABLE document_tag (
    document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    tag_id      INTEGER NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
    provenance  TEXT NOT NULL CHECK (provenance IN ('rule', 'manual', 'removed')),
    PRIMARY KEY (document_id, tag_id)
) WITHOUT ROWID;

-- A rule ignores EITHER a path subtree OR a tag, never both at once: the two
-- are separate rules, and one row meaning both leaves "and/or" unanswerable.
-- A path prefix is relative to a watched root, so it is meaningless without
-- one; a tag rule needs no root and may optionally be confined to one.
CREATE TABLE ignore_rule (
    id              INTEGER PRIMARY KEY,
    watched_root_id INTEGER REFERENCES watched_root(id) ON DELETE CASCADE,
    path_prefix     TEXT,
    tag_id          INTEGER REFERENCES tag(id) ON DELETE CASCADE,
    CHECK ((path_prefix IS NULL) <> (tag_id IS NULL)),
    CHECK (path_prefix IS NULL OR watched_root_id IS NOT NULL)
);

-- ---------------------------------------------------------------- documents

CREATE TABLE document (
    id                  TEXT PRIMARY KEY,   -- sha256 hex of the file bytes
    mime                TEXT NOT NULL,
    size_bytes          INTEGER NOT NULL,
    source_kind         TEXT NOT NULL CHECK (source_kind IN ('document','code','data')),
    parent_document_id  TEXT REFERENCES document(id) ON DELETE SET NULL,
    is_archive          INTEGER NOT NULL DEFAULT 0,
    -- Document-level lifecycle only. Which stage a document reached, and why it
    -- stopped, is `ingest_stage`'s business; this column answers "may it be
    -- searched?" and nothing finer.
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending','indexed','failed','skipped')),
    created_at          INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Several paths may point at one document: content addressing means two copies
-- are one document. A single path column would drop the document when the
-- recorded copy is deleted while the other survives. Requirements §8, D33.
CREATE TABLE path (
    watched_root_id INTEGER NOT NULL REFERENCES watched_root(id) ON DELETE CASCADE,
    relative_path   TEXT NOT NULL,
    document_id     TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    size_bytes      INTEGER NOT NULL,      -- cheap reconciliation without hashing
    mtime           INTEGER NOT NULL,
    PRIMARY KEY (watched_root_id, relative_path)
) WITHOUT ROWID;

CREATE INDEX ix_path_document ON path(document_id);

-- `UNIQUE(id, document_id)` here and on `block` is redundant as a uniqueness
-- claim — `id` alone is already unique — and exists solely to give the composite
-- foreign keys below something to point at. It is what carries `document_id`
-- down the four levels as an enforced fact instead of a convention.
CREATE TABLE page (
    id            INTEGER PRIMARY KEY,
    document_id   TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    page_no       INTEGER NOT NULL,
    -- The vocabulary is the shape `family:detail`, not a closed list: adding an
    -- OCR engine must not cost a migration, but a `text_source` that names no
    -- family at all is a typo, and it would silently match no query ever again.
    -- GLOB, not LIKE: LIKE is case-insensitive for ASCII, so it accepted
    -- 'Native:pdf' and 'OCR:y' — values no reader will ever produce and no
    -- query grouping by family would match again. Free to change only while
    -- nothing has shipped and this file is still edited in place at
    -- SCHEMA_VERSION 1; afterwards it is a migration. Same argument that
    -- removed the tokenchars clause below.
    text_source   TEXT NOT NULL            -- 'native:pdf', 'native:txt', later 'ocr:*'
                  CHECK (text_source GLOB 'native:*' OR text_source GLOB 'ocr:*'),
    section_title TEXT,
    width_px      INTEGER,
    height_px     INTEGER,
    dpi           INTEGER,
    UNIQUE(document_id, page_no),
    UNIQUE(id, document_id)
);

-- `document_id` is denormalised from `page` and pinned to it by the composite
-- foreign key: a block cannot claim a document other than its page's.
CREATE TABLE block (
    id            INTEGER PRIMARY KEY,
    page_id       INTEGER NOT NULL,
    document_id   TEXT NOT NULL,
    -- The server's seven (app/ocr/docling_structurizer.py:29-37) plus `code`,
    -- which only this product can produce: the server never sees a source file.
    type          TEXT NOT NULL CHECK (type IN ('paragraph','headline','caption','table',
                                                'figure','page_header','page_footer','code')),
    reading_order INTEGER NOT NULL,
    language      TEXT,
    script        TEXT,
    confidence    REAL,
    text          TEXT NOT NULL,
    bbox          TEXT,                     -- JSON; populated for PDF, null otherwise
    line_start    INTEGER,                  -- 1-based, inclusive; NULL for formats without lines
    line_end      INTEGER,
    FOREIGN KEY (page_id, document_id) REFERENCES page(id, document_id) ON DELETE CASCADE,
    UNIQUE(id, document_id)
);

-- Reading order is what reconstructs a page, so two blocks may not share a slot.
CREATE UNIQUE INDEX ix_block_page ON block(page_id, reading_order);

-- The chunk's document reference is composite: `(block_id, document_id)` must
-- name a real block OF THAT DOCUMENT. With two independent foreign keys instead,
-- a chunk could carry document A while its block belonged to B, and `citation()`
-- would then name one file and quote a page and section title from another —
-- silently, with no error anywhere. That is the exact failure the four-level
-- model exists to prevent, so it is prevented here rather than in the code.
CREATE TABLE chunk (
    id                   INTEGER PRIMARY KEY,
    document_id          TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    block_id             INTEGER NOT NULL,
    ord                  INTEGER NOT NULL,  -- explicit; the server has none
    text                 TEXT NOT NULL,     -- the original, for display
    -- A JSON array of Segment, one element per source block. `block_id` above is
    -- the first of them, and it is the only one a foreign key can reach — which
    -- is why element 0 is pinned to it here: `citation()` joins through the
    -- column while a highlight measures from the array, and nothing else would
    -- notice the two naming different blocks. The trigger below covers 1..n.
    char_span            TEXT NOT NULL
                         CHECK (json_valid(char_span) AND json_type(char_span) = 'array'
                                AND json_array_length(char_span) >= 1
                                AND json_extract(char_span, '$[0].block_id') = block_id),
    coordinate           TEXT NOT NULL,     -- JSON, tagged enum
    n_chars              INTEGER NOT NULL,
    content_hash         TEXT NOT NULL,
    index_format_version INTEGER NOT NULL,  -- D14: on the row, not in meta
    source_kind          TEXT NOT NULL CHECK (source_kind IN ('document','code','data')),
    UNIQUE(document_id, ord),
    FOREIGN KEY (block_id, document_id) REFERENCES block(id, document_id) ON DELETE CASCADE
);

-- No index on `document_id` alone: UNIQUE(document_id, ord) already leads with it.
CREATE INDEX ix_chunk_block ON chunk(block_id);

-- Blocks 2..n of a chunk live inside char_span, outside ON DELETE CASCADE, and
-- block.id is reused (INTEGER PRIMARY KEY without AUTOINCREMENT, :114). Therefore
-- blocks are deleted only a whole document at a time. Re-extracting a single page
-- would leave ids in char_span that resolve to a live block of another document.
--
-- `NOT EXISTS (… b.id IS json_extract(…))`, and NOT the obvious
-- `json_extract(…) NOT IN (…)`, which is what this trigger said first and what
-- let two bad spans through: an element carrying no `block_id` extracts as NULL,
-- `NULL NOT IN (…)` is NULL rather than true, and the CHECK above missed it too
-- because SQLite counts a CHECK evaluating to NULL as satisfied. Both guards
-- went blind on the same value. `b.id IS <null>` is false for every row.
--
-- The two clauses before EXISTS repeat the CHECK deliberately. A BEFORE INSERT
-- trigger runs AHEAD of the CHECK, so this body must re-establish its own
-- preconditions or json_each() reaches text the CHECK would have refused and
-- raises SQLITE_ERROR — "the statement is broken" — where a caller skipping bad
-- rows expects SQLITE_CONSTRAINT, "this row is bad". Standing the trigger down
-- for anything that is not a JSON array also lets the CHECK report a `char_span`
-- that is a bare object or scalar as what it is, rather than as a span that
-- reached the wrong page.
--
-- `je.type <> 'object'` must stay FIRST in the OR, and is not decoration:
-- json_valid('["hello"]') is 1, so the guard above passes, and json_extract()
-- against that element raises malformed JSON — the same defect one level down,
-- valid container, invalid element. The OR short-circuits, so the type test is
-- what keeps json_extract away from anything it cannot parse. Such an element
-- names no block, so it falls under the message below.
--
-- `_bi`, not `_ai`: the triggers below are AFTER INSERT, this one is BEFORE, and
-- the difference is the whole subject of the paragraph above.
CREATE TRIGGER chunk_span_blocks_bi BEFORE INSERT ON chunk BEGIN
    SELECT RAISE(ABORT, 'char_span names a block outside this chunk''s page')
     WHERE json_valid(new.char_span) AND json_type(new.char_span) = 'array' AND EXISTS (
       SELECT 1 FROM json_each(new.char_span) je
        WHERE je.type <> 'object' OR NOT EXISTS (
          SELECT 1 FROM block b
           WHERE b.id IS json_extract(je.value, '$.block_id')
             AND b.page_id = (SELECT page_id FROM block WHERE id = new.block_id)
             AND b.document_id = new.document_id));
END;

-- Search text is NOT display text: apostrophes unified, ґ→г, ё→е, camelCase
-- expanded for code. Kept apart so a re-embed never rewrites `chunk`. G7.0 §5.4.
CREATE TABLE chunk_search (
    chunk_id INTEGER PRIMARY KEY REFERENCES chunk(id) ON DELETE CASCADE,
    text     TEXT NOT NULL
);

-- ------------------------------------------------------------------ journals

-- The checkpoint a multi-hour job resumes from, keyed on the document's own
-- content hash (D26): the unit of work is one document, so the checkpoint keys
-- on the same thing the transaction does.
--
-- `REFERENCES document(id) ON DELETE CASCADE`, and it is load-bearing rather
-- than tidy. Without it a stage outlived the document it described, and
-- content addressing is what made that dangerous: the key is the *content*, so
-- the hash comes back — an undo, a restore from backup, a file moved out and
-- back. `document_exists` is then false, a fresh document is inserted at
-- `status = 'pending'`, and a stale `done` stage is already sitting over it.
-- Any interruption before the checkpoint leaves `done` over `pending`
-- permanently, because from then on the cheap arm short-circuits on the stage
-- before anything can repair the status. Found twice over, by a scoped review
-- and by the randomised harness, seven of eight seed ranges reaching it.
--
-- In the schema rather than in `delete_document`, because a cascade cannot be
-- forgotten by a caller that is written later. It is the same instrument the
-- four-level model already uses against its other silent failures.
--
-- What it constrains, and this is the part the embedding stages have to know:
-- a stage can no longer be recorded for content that has no `document` row.
-- Every stage this design has is written after the document exists, and a
-- checkpoint for a document that was never inserted is a checkpoint for
-- nothing — but a future stage that wants to check-point *before* the row
-- exists has to insert the row first rather than delete this line.
--
-- The more general shape is a two-phase stage — write `writing` when a stage
-- starts and flip it to `done` when it finishes, so an interrupted stage is
-- distinguishable from an unattempted one rather than merely absent. That was
-- considered and deliberately not taken here: it earns its keep when there are
-- several stages that can each be interrupted, which arrives with embedding,
-- and adding it now would be a vocabulary with one user.
CREATE TABLE ingest_stage (
    content_hash TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    stage        TEXT NOT NULL,
    status       TEXT NOT NULL,
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (content_hash, stage)
) WITHOUT ROWID;

CREATE TABLE skipped (
    id              INTEGER PRIMARY KEY,
    watched_root_id INTEGER REFERENCES watched_root(id) ON DELETE CASCADE,
    relative_path   TEXT NOT NULL,
    page_no         INTEGER,               -- set when a single PDF page was skipped
    reason          TEXT NOT NULL,
    rule            TEXT NOT NULL,         -- which rule fired, so v2 can find the work
    -- Set ONLY for rules that are statements about the file's bytes
    -- (unsupported, no_text_layer, too_large): the same bytes earn the same
    -- verdict, so the next walk must not spend a worker process re-deriving it.
    -- NULL for environmental rules (crash, timeout, memory, unreadable), which
    -- are statements about the machine and have to be retried. D44's asymmetry.
    size_bytes      INTEGER,
    mtime           INTEGER,
    -- Which build's verdict this is. Without it, shipping a reader for a format
    -- that was `unsupported` yesterday would never re-examine the files that
    -- earned the verdict: they would stay unindexed for the life of the index,
    -- and nothing would say so. Bumping INDEX_FORMAT_VERSION when the reader
    -- set changes is therefore an obligation, not a courtesy.
    format_version  INTEGER NOT NULL,
    at              INTEGER NOT NULL DEFAULT (unixepoch())
);

-- One current row per path, not a history.
--
-- COALESCE on both nullable columns, and this is not defensive style: SQLite
-- treats NULLs as DISTINCT inside a UNIQUE index, and `page_no` is NULL for
-- every whole-file skip. A plain UNIQUE(watched_root_id, relative_path, page_no)
-- would therefore dedup nothing at all, and would do it silently.
CREATE UNIQUE INDEX ux_skipped_current ON skipped(
    COALESCE(watched_root_id, -1), relative_path, COALESCE(page_no, -1)
);

-- ------------------------------------------------------------------- vectors

CREATE TABLE model_config (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    provider    TEXT NOT NULL,
    endpoint    TEXT,
    embed_model TEXT NOT NULL,
    dim         INTEGER NOT NULL,
    credential_ref TEXT,                   -- name in the OS credential store, NEVER the key
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- `dim` deliberately duplicates `model_config.dim`: a space keeps the dimension
-- it was built with even after its model config is edited, so the two are
-- allowed to diverge afterwards.
--
-- They are NOT allowed to disagree at the moment a space is created — `space.rs`
-- refuses that, and a test in tests/space.rs pins it. This comment said the
-- opposite until the branch review: true when task 3 wrote it, false from task 5
-- onwards, and by then it read as a licence to skip a check that already exists.
CREATE TABLE embedding_space (
    -- Immutable, and it names the vector table, so it must never be handed out
    -- twice. AUTOINCREMENT is what makes that true: without it SQLite derives a
    -- new id from `max(id) + 1`, which reuses a dropped space's id — and with it
    -- the dropped space's table name — as soon as the space dropped was the
    -- newest, and restarts at 1 once they are all gone. AUTOINCREMENT keeps the
    -- high-water mark in `sqlite_sequence`, advances it for explicitly supplied
    -- ids too, and does not lower it on DELETE.
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    model_config_id      INTEGER NOT NULL REFERENCES model_config(id),
    dim                  INTEGER NOT NULL,
    metric               TEXT NOT NULL DEFAULT 'cosine',
    index_format_version INTEGER NOT NULL,
    chunker_hash         TEXT NOT NULL,
    -- Derived from the immutable id and NEVER from the model name: a vec0 table
    -- corrupts silently on RENAME, so a name that could ever need renaming — a
    -- model name, a dimension — is a defect. The CHECK makes the convention
    -- enforceable rather than remembered. SQLite assigns the rowid before
    -- evaluating it, so this holds even when `id` is omitted from the INSERT.
    vec_table            TEXT NOT NULL UNIQUE CHECK (vec_table = 'vec_emb_' || id),
    state                TEXT NOT NULL CHECK (state IN ('building','ready','stale')),
    created_at           INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(model_config_id, dim, index_format_version, chunker_hash)
);

-- vec0 rejects null vectors, so "not embedded yet" cannot live there. G7.0 §5.7.
CREATE TABLE chunk_embedding_state (
    space_id     INTEGER NOT NULL REFERENCES embedding_space(id) ON DELETE CASCADE,
    chunk_id     INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    state        INTEGER NOT NULL CHECK (state IN (0, 1, 2)),  -- pending, done, failed
    attempts     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (space_id, chunk_id)
) WITHOUT ROWID;

-- ------------------------------------------------------------ lexical index

-- The tokenizer string is fixed by D32 and every clause is paid for by
-- measurement. External content points at chunk_search, not chunk, because the
-- indexed text is a prepared copy and the original is displayed.
--
-- D32 also carried `tokenchars '''’ʼ'` — U+0027, U+2019, U+02BC — and it is gone
-- deliberately. Once the canonical apostrophe became U+02BC, which is a modifier
-- letter and so already inside `L*`, the clause did nothing for prepared text:
-- the other two spellings have been folded away before the tokenizer sees them.
-- For text that did NOT come through prepare_for_search it did worse than
-- nothing — raw `students’ books` indexed as `students’`, and the plain word
-- `students` found nothing — and `index_chunk_text` is public. Removing it is
-- free exactly now: nothing has shipped, so this file is edited in place and
-- SCHEMA_VERSION stays 1. Later it costs a migration and a full reindex.
CREATE VIRTUAL TABLE chunk_fts USING fts5(
    text,
    content='chunk_search',
    content_rowid='chunk_id',
    tokenize="unicode61 remove_diacritics 2 categories 'L* N* Co Mn Mc'"
);

CREATE TRIGGER chunk_search_ai AFTER INSERT ON chunk_search BEGIN
    INSERT INTO chunk_fts(rowid, text) VALUES (new.chunk_id, new.text);
END;

CREATE TRIGGER chunk_search_ad AFTER DELETE ON chunk_search BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text) VALUES ('delete', old.chunk_id, old.text);
END;

CREATE TRIGGER chunk_search_au AFTER UPDATE ON chunk_search BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text) VALUES ('delete', old.chunk_id, old.text);
    INSERT INTO chunk_fts(rowid, text) VALUES (new.chunk_id, new.text);
END;
