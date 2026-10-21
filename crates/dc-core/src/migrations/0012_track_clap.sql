-- CLAP audio embeddings per track (crate::clap): 512 values of a unit
-- vector, quantised to i16 (value × 32767), used for text search.
CREATE TABLE track_clap (
    track_id      TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    model_version TEXT NOT NULL,
    data          BLOB NOT NULL
);
