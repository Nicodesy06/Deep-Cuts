-- Energy written by other software, e.g. Mixed In Key's "Energy 7" in a tag
-- or comment: used to compare and calibrate Cradex's energy (dna.rs).
CREATE TABLE track_energy_ref (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    value    INTEGER NOT NULL CHECK (value BETWEEN 1 AND 10),
    source   TEXT NOT NULL
);
-- Embeddings of optional audio models (models.rs), one row per track and
-- model: little-endian f32 values, mean over the analysed windows.
CREATE TABLE track_embedding (
    track_id      TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    model         TEXT NOT NULL,
    model_version TEXT NOT NULL,
    data          BLOB NOT NULL,
    PRIMARY KEY (track_id, model)
);
