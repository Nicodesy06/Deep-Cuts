-- Energy 1–10: rank of the measured energy proxy within the library
-- (dna::update_energy). NULL until analysed.
ALTER TABLE tracks ADD COLUMN energy INTEGER;
DROP VIEW live_tracks;
CREATE VIEW live_tracks AS SELECT * FROM tracks WHERE archived = 0;
