-- Compact "sound" vector per track: the local audio model's clip embedding
-- projected to 32 dimensions (crate::ml::sound_vector). Used to find tracks
-- that sound alike (similarity, genre/tag suggestions).
CREATE TABLE track_sound (
    track_id      TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    model         TEXT NOT NULL,
    model_version TEXT NOT NULL,
    data          BLOB NOT NULL    -- 32 little-endian f32
);
