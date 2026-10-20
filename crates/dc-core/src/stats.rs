//! Library statistics for the dashboard. Everything is computed from the
//! live database — there is no demo data.

use crate::error::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub label: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub total_tracks: i64,
    pub total_duration_ms: i64,
    pub total_size_bytes: i64,
    pub analyzed: i64,
    pub analysis_failed: i64,
    pub missing_files: i64,
    pub keep: i64,
    pub pass: i64,
    pub maybe: i64,
    pub undecided: i64,
    pub without_genre: i64,
    pub without_bpm: i64,
    pub without_key: i64,
    pub without_artwork: i64,
    pub unrated: i64,
    pub not_in_playlist: i64,
    pub playlists: i64,
    pub duplicate_hash_groups: i64,
    pub added_last_7_days: i64,
    pub genres: Vec<Bucket>,
    pub bpm: Vec<Bucket>,
    pub keys: Vec<Bucket>,
    pub formats: Vec<Bucket>,
    pub loudness: Vec<Bucket>,
    pub growth: Vec<Bucket>,
}

fn buckets(conn: &Connection, sql: &str) -> Result<Vec<Bucket>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(Bucket {
            label: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            count: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn count(conn: &Connection, sql: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [], |r| r.get(0))?)
}

pub fn library_stats(conn: &Connection) -> Result<LibraryStats> {
    let week_ago = crate::util::now_ms() - 7 * 86_400_000;
    Ok(LibraryStats {
        total_tracks: count(conn, "SELECT count(*) FROM tracks")?,
        total_duration_ms: count(conn, "SELECT COALESCE(sum(duration_ms), 0) FROM tracks")?,
        total_size_bytes: count(conn, "SELECT COALESCE(sum(size), 0) FROM files")?,
        analyzed: count(conn, "SELECT count(*) FROM tracks WHERE analysis_status = 'done'")?,
        analysis_failed: count(conn, "SELECT count(*) FROM tracks WHERE analysis_status = 'failed'")?,
        missing_files: count(conn, "SELECT count(*) FROM files WHERE missing = 1")?,
        keep: count(conn, "SELECT count(*) FROM decisions WHERE context = 'global' AND decision = 'keep'")?,
        pass: count(conn, "SELECT count(*) FROM decisions WHERE context = 'global' AND decision = 'pass'")?,
        maybe: count(conn, "SELECT count(*) FROM decisions WHERE context = 'global' AND decision = 'maybe'")?,
        undecided: count(
            conn,
            "SELECT count(*) FROM tracks t WHERE NOT EXISTS (SELECT 1 FROM decisions d WHERE d.track_id = t.id AND d.context = 'global')",
        )?,
        without_genre: count(conn, "SELECT count(*) FROM tracks WHERE genre IS NULL OR genre = ''")?,
        without_bpm: count(conn, "SELECT count(*) FROM tracks WHERE bpm IS NULL")?,
        without_key: count(conn, "SELECT count(*) FROM tracks WHERE key_camelot IS NULL")?,
        without_artwork: count(conn, "SELECT count(*) FROM tracks WHERE has_artwork = 0")?,
        unrated: count(conn, "SELECT count(*) FROM tracks WHERE rating IS NULL OR rating = 0")?,
        not_in_playlist: count(
            conn,
            "SELECT count(*) FROM tracks t WHERE NOT EXISTS (SELECT 1 FROM playlist_tracks p WHERE p.track_id = t.id)",
        )?,
        playlists: count(conn, "SELECT count(*) FROM playlists WHERE kind != 'folder'")?,
        duplicate_hash_groups: count(
            conn,
            "SELECT count(*) FROM (SELECT quick_hash FROM files WHERE quick_hash IS NOT NULL GROUP BY quick_hash HAVING count(DISTINCT track_id) > 1)",
        )?,
        added_last_7_days: conn.query_row("SELECT count(*) FROM tracks WHERE created_at >= ?1", [week_ago], |r| r.get(0))?,
        genres: buckets(
            conn,
            "SELECT genre, count(*) c FROM tracks WHERE genre IS NOT NULL AND genre != '' GROUP BY genre COLLATE NOCASE ORDER BY c DESC LIMIT 15",
        )?,
        bpm: buckets(
            conn,
            "SELECT CAST(CAST(bpm / 2 AS INTEGER) * 2 AS TEXT), count(*) FROM tracks WHERE bpm BETWEEN 60 AND 200
             GROUP BY CAST(bpm / 2 AS INTEGER) ORDER BY CAST(bpm / 2 AS INTEGER)",
        )?,
        keys: buckets(
            conn,
            "SELECT key_camelot, count(*) FROM tracks WHERE key_camelot IS NOT NULL GROUP BY key_camelot
             ORDER BY CAST(rtrim(key_camelot, 'AB') AS INTEGER), key_camelot",
        )?,
        formats: buckets(conn, "SELECT upper(format), count(*) c FROM files GROUP BY upper(format) ORDER BY c DESC")?,
        loudness: buckets(
            conn,
            // floor() for negative values without the math extension
            "SELECT CAST(b AS TEXT), count(*) FROM (SELECT CAST(lufs AS INTEGER) - (lufs < CAST(lufs AS INTEGER)) b
             FROM tracks WHERE lufs IS NOT NULL) GROUP BY b ORDER BY b",
        )?,
        growth: buckets(
            conn,
            "SELECT strftime('%Y-%m', created_at / 1000, 'unixepoch'), count(*) FROM tracks GROUP BY 1 ORDER BY 1",
        )?,
    })
}
