-- Outputs of the local audio tagging model (see crate::ml): the kept
-- music-related AudioSet classes per track, with their probability.
CREATE TABLE model_tags (
    track_id      TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    model         TEXT NOT NULL,
    model_version TEXT NOT NULL,
    grp           TEXT NOT NULL,   -- genre | mood | vocals | instrument
    label         TEXT NOT NULL,
    score         REAL NOT NULL,
    rank          INTEGER NOT NULL, -- 1 = highest score within the group
    PRIMARY KEY (track_id, model, label)
);
CREATE INDEX idx_model_tags_label ON model_tags(model, label, rank);
