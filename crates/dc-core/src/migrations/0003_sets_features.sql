-- Schema v3: measured feature vectors (similarity / map) and set preparation.

-- Fixed-layout f32 vector derived from the analyser output (see features.rs).
CREATE TABLE track_features (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    version  INTEGER NOT NULL,
    data     BLOB NOT NULL
);

-- Chapters of a set (kind = 'set' playlists): a chapter starts at a
-- position and lasts until the next chapter.
CREATE TABLE set_chapters (
    id          TEXT PRIMARY KEY,
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    role        TEXT NOT NULL,   -- opening|early-warmup|building|late-warmup|peak|closing|custom
    name        TEXT,
    UNIQUE (playlist_id, position)
);

-- Tracks pinned in a set: Magic Sort never moves them.
CREATE TABLE set_locks (
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    PRIMARY KEY (playlist_id, track_id)
);

ALTER TABLE playlists ADD COLUMN notes TEXT;
