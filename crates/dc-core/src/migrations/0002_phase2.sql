-- Schema v2: undo/redo history, tag synonyms + quick-tag slots, watch folders.

-- One batch per user action (bulk edit, smart fix, decision, …); `changes`
-- holds the before/after state so the batch can be undone and redone.
CREATE TABLE change_batches (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    ts      INTEGER NOT NULL,
    action  TEXT NOT NULL,
    summary TEXT,
    undone  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE changes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id    INTEGER NOT NULL REFERENCES change_batches(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,      -- field|decision|notes|favorite|tag
    track_id    TEXT NOT NULL,
    key         TEXT NOT NULL,      -- field name, decision context or tag id
    old_present INTEGER NOT NULL,
    old_value   TEXT,
    new_present INTEGER NOT NULL,
    new_value   TEXT
);
CREATE INDEX idx_changes_batch ON changes(batch_id);
CREATE INDEX idx_changes_track ON changes(track_id);

CREATE TABLE tag_synonyms (
    synonym TEXT NOT NULL COLLATE NOCASE PRIMARY KEY,
    tag_id  TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE
);

-- Keyboard Quick Tag slots (Shift+1 … Shift+9).
CREATE TABLE quick_tags (
    slot   INTEGER PRIMARY KEY CHECK (slot BETWEEN 1 AND 9),
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE
);

CREATE TABLE watch_folders (
    path      TEXT PRIMARY KEY,
    added_at  INTEGER NOT NULL,
    last_scan INTEGER
);

CREATE INDEX idx_track_tags_tag ON track_tags(tag_id, status);
