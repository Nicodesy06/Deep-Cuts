-- Acoustic fingerprints (analysis::fingerprint), one per analysed track.
CREATE TABLE fingerprints (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    version  INTEGER NOT NULL,
    data     BLOB NOT NULL
);
