-- Discogs style model outputs per track (crate::discogs): the 400 style
-- logits, quantised to i16 (value / 1000), used to compare tracks by style.
CREATE TABLE track_styles (
    track_id      TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    model_version TEXT NOT NULL,
    data          BLOB NOT NULL
);
