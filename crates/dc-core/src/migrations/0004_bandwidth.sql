-- Measured audio bandwidth and transcoding suspicion (analysis 1.2.0).
ALTER TABLE tracks ADD COLUMN cutoff_hz REAL;
-- NULL = no suspicion; otherwise a rule id from analysis::bandwidth.
ALTER TABLE tracks ADD COLUMN transcode TEXT;
CREATE INDEX idx_tracks_transcode ON tracks(transcode) WHERE transcode IS NOT NULL;
