-- Archived tracks are hidden from the library (and from searches, stats,
-- duplicates and suggestions) without being deleted; `archived:yes` shows them.
ALTER TABLE tracks ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_tracks_archived ON tracks(archived) WHERE archived = 1;
CREATE VIEW live_tracks AS SELECT * FROM tracks WHERE archived = 0;
