//! Library core: tracks, files, metadata provenance, decisions, playlists.

use crate::error::{Error, Result};
use crate::history;
use crate::metadata::{self, FileMetadata};
use crate::util::{new_id, now_ms};
use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Metadata fields & provenance
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Int,
    Real,
}

/// Editable metadata fields: (field name, column on `tracks`, kind).
const FIELDS: &[(&str, &str, Kind)] = &[
    ("title", "title", Kind::Text),
    ("artist", "artist", Kind::Text),
    ("version", "version", Kind::Text),
    ("remixer", "remixer", Kind::Text),
    ("album", "album", Kind::Text),
    ("albumArtist", "album_artist", Kind::Text),
    ("label", "label", Kind::Text),
    ("genre", "genre", Kind::Text),
    ("subgenre", "subgenre", Kind::Text),
    ("year", "year", Kind::Int),
    ("bpm", "bpm", Kind::Real),
    ("key", "musical_key", Kind::Text),
    ("comment", "comment", Kind::Text),
    ("isrc", "isrc", Kind::Text),
    ("durationMs", "duration_ms", Kind::Int),
    ("rating", "rating", Kind::Int),
];

pub fn field_names() -> impl Iterator<Item = &'static str> {
    FIELDS.iter().map(|f| f.0)
}

fn field_def(field: &str) -> Result<(&'static str, &'static str, Kind)> {
    FIELDS
        .iter()
        .copied()
        .find(|f| f.0 == field)
        .ok_or_else(|| Error::Invalid(format!("unknown field '{field}'")))
}

/// Lower is stronger. User edits always win; generated values never
/// override anything that came from a human or a file.
fn source_rank(source: &str) -> i32 {
    match source {
        "user" => 0,
        s if s.starts_with("import:") => 1,
        "file" => 2,
        "analysis" => 3,
        _ => 4,
    }
}

pub const SOURCE_FILE: &str = "file";
pub const SOURCE_USER: &str = "user";
pub const SOURCE_ANALYSIS: &str = "analysis";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldValue {
    pub field: String,
    pub source: String,
    pub value: Option<String>,
    pub updated_at: i64,
}

/// Store a value for (track, field, source) without touching other sources,
/// then re-resolve the effective value.
pub fn set_field(
    conn: &Connection,
    track_id: &str,
    field: &str,
    source: &str,
    value: Option<&str>,
) -> Result<()> {
    let (_, _, kind) = field_def(field)?;
    let value = match (kind, value.map(str::trim).filter(|v| !v.is_empty())) {
        (_, None) => None,
        (Kind::Text, Some(v)) => Some(v.to_string()),
        (Kind::Int, Some(v)) => Some(
            v.parse::<f64>()
                .map_err(|_| Error::Invalid(format!("{field}: '{v}' is not a number")))?
                .round()
                .to_string(),
        ),
        (Kind::Real, Some(v)) => Some(
            v.replace(',', ".")
                .parse::<f64>()
                .map_err(|_| Error::Invalid(format!("{field}: '{v}' is not a number")))?
                .to_string(),
        ),
    };
    if field == "rating"
        && let Some(r) = &value
    {
        let r: i64 = r.parse().unwrap_or(-1);
        if !(0..=5).contains(&r) {
            return Err(Error::Invalid("rating must be 0..5".into()));
        }
    }
    conn.execute(
        "INSERT INTO track_fields (track_id, field, source, value, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(track_id, field, source) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![track_id, field, source, value, now_ms()],
    )?;
    resolve_field(conn, track_id, field)
}

/// Remove one source's value (e.g. "revert my edit to the file value").
pub fn clear_field_source(
    conn: &Connection,
    track_id: &str,
    field: &str,
    source: &str,
) -> Result<()> {
    field_def(field)?;
    conn.execute(
        "DELETE FROM track_fields WHERE track_id = ?1 AND field = ?2 AND source = ?3",
        params![track_id, field, source],
    )?;
    resolve_field(conn, track_id, field)
}

pub fn field_values(conn: &Connection, track_id: &str) -> Result<Vec<FieldValue>> {
    let mut stmt = conn.prepare(
        "SELECT field, source, value, updated_at FROM track_fields WHERE track_id = ?1 ORDER BY field, source",
    )?;
    let rows = stmt.query_map([track_id], |r| {
        Ok(FieldValue {
            field: r.get(0)?,
            source: r.get(1)?,
            value: r.get(2)?,
            updated_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn resolve_field(conn: &Connection, track_id: &str, field: &str) -> Result<()> {
    let (_, column, kind) = field_def(field)?;
    let mut stmt = conn.prepare_cached(
        "SELECT source, value FROM track_fields WHERE track_id = ?1 AND field = ?2",
    )?;
    let mut best: Option<(i32, Option<String>)> = None;
    let rows = stmt.query_map(params![track_id, field], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    for row in rows {
        let (source, value) = row?;
        let rank = source_rank(&source);
        if best.as_ref().is_none_or(|(b, _)| rank < *b) {
            best = Some((rank, value));
        }
    }
    let value = best.and_then(|b| b.1);
    let sql = format!("UPDATE tracks SET {column} = ?1, updated_at = ?2 WHERE id = ?3");
    match kind {
        Kind::Text => conn.execute(&sql, params![value, now_ms(), track_id])?,
        Kind::Int => conn.execute(
            &sql,
            params![
                value.and_then(|v| v.parse::<i64>().ok()),
                now_ms(),
                track_id
            ],
        )?,
        Kind::Real => conn.execute(
            &sql,
            params![
                value.and_then(|v| v.parse::<f64>().ok()),
                now_ms(),
                track_id
            ],
        )?,
    };
    if field == "key" {
        conn.execute(
            "UPDATE tracks SET key_camelot = ?1 WHERE id = ?2",
            params![
                conn.query_row(
                    "SELECT musical_key FROM tracks WHERE id = ?1",
                    [track_id],
                    |r| r.get::<_, Option<String>>(0)
                )?
                .as_deref()
                .and_then(crate::keys::to_camelot),
                track_id
            ],
        )?;
    }
    Ok(())
}

/// Rebuild the full-text row for a track.
pub fn refresh_fts(conn: &Connection, track_id: &str) -> Result<()> {
    let rowid: Option<i64> = conn
        .query_row("SELECT rowid FROM tracks WHERE id = ?1", [track_id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(rowid) = rowid else { return Ok(()) };
    conn.execute("DELETE FROM tracks_fts WHERE rowid = ?1", [rowid])?;
    conn.execute(
        "INSERT INTO tracks_fts (rowid, title, artist, version, remixer, album, label, genre, comment, notes, path)
         SELECT t.rowid, t.title, t.artist, t.version, t.remixer, t.album, t.label, t.genre, t.comment, t.notes,
                (SELECT group_concat(path, ' ') FROM files WHERE track_id = t.id)
         FROM tracks t WHERE t.id = ?1",
        [track_id],
    )?;
    Ok(())
}

pub fn audit(
    conn: &Connection,
    action: &str,
    entity: &str,
    entity_id: Option<&str>,
    details: serde_json::Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO audit_log (ts, action, entity, entity_id, details) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now_ms(), action, entity, entity_id, details.to_string()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub scanned: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub relinked: usize,
    pub failed: Vec<ImportFailure>,
    pub added_track_ids: Vec<String>,
    pub elapsed_ms: i64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub phase: &'static str, // scanning | reading
    pub done: usize,
    pub total: usize,
}

/// Fast content identity: blake3 over size + first and last 256 KiB. Stable
/// across moves/renames, which is what relinking needs.
pub fn quick_hash(path: &Path) -> Result<String> {
    const CHUNK: u64 = 256 * 1024;
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());
    let mut buf = vec![0u8; CHUNK as usize];
    let n = f.read(&mut buf)?;
    h.update(&buf[..n]);
    if size > CHUNK * 2 {
        f.seek(SeekFrom::Start(size - CHUNK))?;
        let mut n = 0;
        while n < buf.len() {
            let r = f.read(&mut buf[n..])?;
            if r == 0 {
                break;
            }
            n += r;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex()[..32].to_string())
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

pub fn collect_audio_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root.is_file() {
            if crate::util::is_audio_file(root) {
                out.push(root.clone());
            }
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            let hidden = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("._"));
            if entry.file_type().is_file() && !hidden && crate::util::is_audio_file(p) {
                out.push(p.to_path_buf());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn path_key(p: &Path) -> String {
    // Canonicalise when possible so the same file is never imported twice
    // through different relative paths.
    dunce::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

struct Scanned {
    path: String,
    size: i64,
    mtime: Option<i64>,
    result: std::result::Result<(FileMetadata, String), String>,
}

/// Import (or refresh) audio files under `roots`. Read-only on the files.
/// Incremental: unchanged files (same size + mtime) are skipped, moved files
/// are relinked to their existing track by content hash.
pub fn import_paths(
    conn: &mut Connection,
    roots: &[PathBuf],
    cancel: Option<Arc<AtomicBool>>,
    progress: &mut dyn FnMut(ImportProgress),
) -> Result<ImportReport> {
    let started = now_ms();
    let mut report = ImportReport::default();
    progress(ImportProgress {
        phase: "scanning",
        done: 0,
        total: 0,
    });
    let files = collect_audio_files(roots);
    report.scanned = files.len();

    let mut known: HashMap<String, (String, i64, Option<i64>)> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT path, id, size, mtime_ms FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
        })?;
        for row in rows {
            let (p, v) = row?;
            known.insert(p, v);
        }
    }

    let total = files.len();
    let mut done = 0;
    for chunk in files.chunks(128) {
        if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            report.cancelled = true;
            break;
        }
        let now = now_ms();
        let scanned: Vec<Option<Scanned>> = chunk
            .par_iter()
            .map(|p| {
                let key = path_key(p);
                let meta = std::fs::metadata(p).ok()?;
                let size = meta.len() as i64;
                let mtime = mtime_ms(&meta);
                if let Some((_, ksize, kmtime)) = known.get(&key)
                    && *ksize == size
                    && *kmtime == mtime
                {
                    return Some(Scanned {
                        path: key,
                        size,
                        mtime,
                        result: Err(String::new()),
                    });
                }
                let result = metadata::read(p)
                    .map_err(|e| e.to_string())
                    .and_then(|m| quick_hash(p).map(|h| (m, h)).map_err(|e| e.to_string()));
                Some(Scanned {
                    path: key,
                    size,
                    mtime,
                    result,
                })
            })
            .collect();

        let tx = crate::util::write_tx(conn)?;
        for s in scanned.into_iter().flatten() {
            match (&s.result, known.get(&s.path)) {
                (Err(e), Some((file_id, _, _))) if e.is_empty() => {
                    tx.execute(
                        "UPDATE files SET last_seen = ?1, missing = 0 WHERE id = ?2",
                        params![now, file_id],
                    )?;
                    report.unchanged += 1;
                }
                (Err(e), _) => report.failed.push(ImportFailure {
                    path: s.path.clone(),
                    error: e.clone(),
                }),
                (Ok((meta, hash)), Some((file_id, _, _))) => {
                    let track_id: String =
                        tx.query_row("SELECT track_id FROM files WHERE id = ?1", [file_id], |r| {
                            r.get(0)
                        })?;
                    update_file_row(&tx, file_id, &s, meta, hash, now)?;
                    apply_file_metadata(&tx, &track_id, meta)?;
                    refresh_fts(&tx, &track_id)?;
                    report.updated += 1;
                }
                (Ok((meta, hash)), None) => {
                    // A missing file with identical content => the file moved.
                    let moved: Option<(String, String)> = tx
                        .query_row(
                            "SELECT id, track_id FROM files WHERE quick_hash = ?1 AND size = ?2 AND missing = 1 LIMIT 1",
                            params![hash, s.size],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    if let Some((file_id, track_id)) = moved {
                        let old: String = tx.query_row(
                            "SELECT path FROM files WHERE id = ?1",
                            [&file_id],
                            |r| r.get(0),
                        )?;
                        tx.execute(
                            "UPDATE files SET path = ?1 WHERE id = ?2",
                            params![s.path, file_id],
                        )?;
                        update_file_row(&tx, &file_id, &s, meta, hash, now)?;
                        refresh_fts(&tx, &track_id)?;
                        audit(
                            &tx,
                            "relink",
                            "file",
                            Some(&file_id),
                            serde_json::json!({"from": old, "to": s.path}),
                        )?;
                        report.relinked += 1;
                    } else {
                        let track_id = new_id();
                        tx.execute(
                            "INSERT INTO tracks (id, created_at, updated_at, has_artwork) VALUES (?1, ?2, ?2, ?3)",
                            params![track_id, now, meta.has_artwork],
                        )?;
                        tx.execute(
                            "INSERT INTO files (id, track_id, path, size, mtime_ms, quick_hash, format, bitrate, sample_rate, bit_depth, channels, last_seen, added_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                            params![new_id(), track_id, s.path, s.size, s.mtime, hash, meta.format, meta.bitrate, meta.sample_rate, meta.bit_depth, meta.channels, now],
                        )?;
                        apply_file_metadata(&tx, &track_id, meta)?;
                        refresh_fts(&tx, &track_id)?;
                        report.added_track_ids.push(track_id);
                        report.added += 1;
                    }
                }
            }
        }
        tx.commit()?;
        done += chunk.len();
        progress(ImportProgress {
            phase: "reading",
            done,
            total,
        });
    }
    report.elapsed_ms = now_ms() - started;
    audit(
        conn,
        "import",
        "library",
        None,
        serde_json::json!({
            "roots": roots, "scanned": report.scanned, "added": report.added, "updated": report.updated,
            "relinked": report.relinked, "failed": report.failed.len()
        }),
    )?;
    Ok(report)
}

fn update_file_row(
    conn: &Connection,
    file_id: &str,
    s: &Scanned,
    meta: &FileMetadata,
    hash: &str,
    now: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE files SET size = ?1, mtime_ms = ?2, quick_hash = ?3, format = ?4, bitrate = ?5, sample_rate = ?6,
                bit_depth = ?7, channels = ?8, missing = 0, last_seen = ?9 WHERE id = ?10",
        params![s.size, s.mtime, hash, meta.format, meta.bitrate, meta.sample_rate, meta.bit_depth, meta.channels, now, file_id],
    )?;
    Ok(())
}

/// Write the file's own tags as source='file'. Other sources are untouched.
pub(crate) fn apply_file_metadata(
    conn: &Connection,
    track_id: &str,
    m: &FileMetadata,
) -> Result<()> {
    let num = |v: Option<i64>| v.map(|v| v.to_string());
    let values: [(&str, Option<String>); 15] = [
        ("title", m.title.clone()),
        ("artist", m.artist.clone()),
        ("version", m.version.clone()),
        ("remixer", m.remixer.clone()),
        ("album", m.album.clone()),
        ("albumArtist", m.album_artist.clone()),
        ("label", m.label.clone()),
        ("genre", m.genre.clone()),
        ("year", num(m.year)),
        ("bpm", m.bpm.map(|b| b.to_string())),
        ("key", m.musical_key.clone()),
        ("comment", m.comment.clone()),
        ("isrc", m.isrc.clone()),
        ("durationMs", num(m.duration_ms)),
        ("rating", None),
    ];
    for (field, value) in values {
        if field == "rating" {
            continue;
        }
        match value {
            Some(v) => set_field(conn, track_id, field, SOURCE_FILE, Some(&v))?,
            None => clear_field_source(conn, track_id, field, SOURCE_FILE)?,
        }
    }
    conn.execute(
        "UPDATE tracks SET has_artwork = ?1 WHERE id = ?2",
        params![m.has_artwork, track_id],
    )?;
    Ok(())
}

/// Mark files missing/present. Returns (checked, missing).
pub fn check_missing_files(conn: &Connection) -> Result<(usize, usize)> {
    let files: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, path FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let flags: Vec<bool> = files
        .par_iter()
        .map(|(_, p)| !Path::new(p).exists())
        .collect();
    let missing = crate::util::atomic(conn, |tx| {
        let mut missing = 0;
        for ((id, _), is_missing) in files.iter().zip(&flags) {
            missing += *is_missing as usize;
            tx.execute(
                "UPDATE files SET missing = ?1 WHERE id = ?2",
                params![is_missing, id],
            )?;
        }
        Ok(missing)
    })?;
    Ok((files.len(), missing))
}

// ---------------------------------------------------------------------------
// Tracks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub version: Option<String>,
    pub remixer: Option<String>,
    pub album: Option<String>,
    pub label: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub duration_ms: Option<i64>,
    pub rating: Option<i64>,
    pub favorite: bool,
    pub decision: Option<String>,
    pub comment: Option<String>,
    pub path: Option<String>,
    pub format: Option<String>,
    pub bitrate: Option<i64>,
    pub missing: bool,
    pub lufs: Option<f64>,
    pub preview_ms: Option<i64>,
    pub analysis_status: String,
    pub created_at: i64,
}

pub(crate) const TRACK_SELECT: &str = "SELECT t.id, t.title, t.artist, t.version, t.remixer, t.album, t.label, t.genre, t.year,
    t.bpm, t.musical_key, t.key_camelot, t.duration_ms, t.rating, t.favorite,
    (SELECT decision FROM decisions d WHERE d.track_id = t.id AND d.context = 'global'),
    t.comment, f.path, f.format, f.bitrate, COALESCE(f.missing, 1), t.lufs, t.preview_ms, t.analysis_status, t.created_at
    FROM tracks t
    LEFT JOIN files f ON f.id = (SELECT id FROM files WHERE track_id = t.id ORDER BY missing, added_at LIMIT 1)";

pub(crate) fn row_to_track(r: &rusqlite::Row) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        version: r.get(3)?,
        remixer: r.get(4)?,
        album: r.get(5)?,
        label: r.get(6)?,
        genre: r.get(7)?,
        year: r.get(8)?,
        bpm: r.get(9)?,
        key: r.get(10)?,
        camelot: r.get(11)?,
        duration_ms: r.get(12)?,
        rating: r.get(13)?,
        favorite: r.get(14)?,
        decision: r.get(15)?,
        comment: r.get(16)?,
        path: r.get(17)?,
        format: r.get(18)?,
        bitrate: r.get(19)?,
        missing: r.get(20)?,
        lufs: r.get(21)?,
        preview_ms: r.get(22)?,
        analysis_status: r.get(23)?,
        created_at: r.get(24)?,
    })
}

pub fn get_track(conn: &Connection, id: &str) -> Result<TrackRow> {
    conn.query_row(
        &format!("{TRACK_SELECT} WHERE t.id = ?1"),
        [id],
        row_to_track,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("track {id}")))
}

pub fn track_path(conn: &Connection, id: &str) -> Result<PathBuf> {
    let p: Option<String> = conn
        .query_row(
            "SELECT path FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    p.map(PathBuf::from)
        .ok_or_else(|| Error::NotFound(format!("file for track {id}")))
}

pub fn track_notes(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT notes FROM tracks WHERE id = ?1", [id], |r| r.get(0))?)
}

pub fn set_notes(conn: &Connection, id: &str, notes: Option<&str>) -> Result<()> {
    let notes = notes.map(str::trim).filter(|n| !n.is_empty());
    history::batch(conn, "notes", "notes", |tx, b| {
        history::change(
            tx,
            b,
            history::Kind::Notes,
            id,
            "notes",
            history::State::set(notes),
        )?;
        Ok(())
    })
}

pub fn set_favorite(conn: &Connection, ids: &[String], favorite: bool) -> Result<()> {
    let v = if favorite { "1" } else { "0" };
    history::batch(
        conn,
        "favorite",
        &format!("favorite={favorite}"),
        |tx, b| {
            for id in ids {
                history::change(
                    tx,
                    b,
                    history::Kind::Favorite,
                    id,
                    "favorite",
                    history::State::set(Some(v)),
                )?;
            }
            Ok(())
        },
    )
}

/// User edit of a metadata field (stored as source='user'; never
/// overwritten by import or analysis). Undoable as one batch.
pub fn edit_field(
    conn: &Connection,
    ids: &[String],
    field: &str,
    value: Option<&str>,
) -> Result<()> {
    field_def(field)?;
    history::batch(
        conn,
        "edit",
        &format!("{field} = {}", value.unwrap_or("∅")),
        |tx, b| {
            for id in ids {
                history::change(
                    tx,
                    b,
                    history::Kind::Field,
                    id,
                    field,
                    history::State::set(value),
                )?;
            }
            audit(
                tx,
                "edit",
                "track",
                None,
                serde_json::json!({"ids": ids, "field": field, "value": value}),
            )
        },
    )
}

/// Remove the user's value so the import/file/analysis value applies again.
pub fn revert_field(conn: &Connection, ids: &[String], field: &str) -> Result<()> {
    field_def(field)?;
    history::batch(conn, "revert", &format!("revert {field}"), |tx, b| {
        for id in ids {
            history::change(
                tx,
                b,
                history::Kind::Field,
                id,
                field,
                history::State::absent(),
            )?;
        }
        audit(
            tx,
            "revert",
            "track",
            None,
            serde_json::json!({"ids": ids, "field": field}),
        )
    })
}

// ---------------------------------------------------------------------------
// Decisions
// ---------------------------------------------------------------------------

pub const DECISIONS: &[&str] = &["keep", "pass", "maybe"];

/// Set (or clear with None) a Keep/Pass/Maybe decision in a context.
pub fn set_decision(
    conn: &Connection,
    ids: &[String],
    context: &str,
    decision: Option<&str>,
) -> Result<()> {
    if let Some(d) = decision
        && !DECISIONS.contains(&d)
    {
        return Err(Error::Invalid(format!("unknown decision '{d}'")));
    }
    if context.trim().is_empty() {
        return Err(Error::Invalid("decision context is empty".into()));
    }
    let state = match decision {
        Some(d) => history::State::set(Some(d)),
        None => history::State::absent(),
    };
    history::batch(
        conn,
        "decision",
        &format!("{} ({context})", decision.unwrap_or("clear")),
        |tx, b| {
            for id in ids {
                history::change(tx, b, history::Kind::Decision, id, context, state.clone())?;
            }
            audit(
                tx,
                "decision",
                "track",
                None,
                serde_json::json!({"ids": ids, "context": context, "decision": decision}),
            )
        },
    )
}

// ---------------------------------------------------------------------------
// Playlists
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub rules: Option<String>,
    pub source: Option<String>,
    pub track_count: i64,
}

pub fn create_playlist(
    conn: &Connection,
    name: &str,
    parent_id: Option<&str>,
    kind: &str,
    rules: Option<&str>,
) -> Result<String> {
    if !["manual", "smart", "folder", "set"].contains(&kind) {
        return Err(Error::Invalid(format!("unknown playlist kind '{kind}'")));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("playlist name is empty".into()));
    }
    if let Some(r) = rules {
        crate::query::compile(r)?; // validate
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO playlists (id, name, parent_id, kind, rules, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, name, parent_id, kind, rules, now_ms()],
    )?;
    Ok(id)
}

pub fn list_playlists(conn: &Connection) -> Result<Vec<Playlist>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.parent_id, p.kind, p.rules, p.source,
                (SELECT count(*) FROM playlist_tracks pt WHERE pt.playlist_id = p.id)
         FROM playlists p ORDER BY p.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Playlist {
            id: r.get(0)?,
            name: r.get(1)?,
            parent_id: r.get(2)?,
            kind: r.get(3)?,
            rules: r.get(4)?,
            source: r.get(5)?,
            track_count: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn rename_playlist(conn: &Connection, id: &str, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name.trim(), now_ms(), id],
    )?;
    Ok(())
}

pub fn delete_playlist(conn: &Connection, id: &str) -> Result<()> {
    audit(
        conn,
        "delete",
        "playlist",
        Some(id),
        serde_json::json!({"tracks": playlist_track_ids(conn, id)?}),
    )?;
    conn.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
    Ok(())
}

pub fn playlist_track_ids(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Replace the playlist order with `track_ids` (duplicates allowed).
pub fn set_playlist_tracks(conn: &Connection, id: &str, track_ids: &[String]) -> Result<()> {
    crate::util::atomic(conn, |tx| {
        tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [id])?;
        for (i, t) in track_ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                params![id, i as i64, t],
            )?;
        }
        tx.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![now_ms(), id],
        )?;
        Ok(())
    })
}

/// User reorder / content change of a playlist, as one undoable batch.
pub fn reorder_playlist(
    conn: &Connection,
    id: &str,
    track_ids: &[String],
    action: &str,
) -> Result<()> {
    let name: String = conn
        .query_row("SELECT name FROM playlists WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("playlist {id}")))?;
    history::batch(conn, action, &format!("{action}: {name}"), |tx, b| {
        history::change(
            tx,
            b,
            history::Kind::PlaylistOrder,
            id,
            id,
            history::State::set(Some(&serde_json::to_string(track_ids)?)),
        )?;
        Ok(())
    })
}

pub fn add_to_playlist(conn: &Connection, id: &str, track_ids: &[String]) -> Result<()> {
    let mut all = playlist_track_ids(conn, id)?;
    for t in track_ids {
        if !all.contains(t) {
            all.push(t.clone());
        }
    }
    reorder_playlist(conn, id, &all, "playlist-add")
}

pub fn remove_from_playlist(conn: &Connection, id: &str, track_ids: &[String]) -> Result<()> {
    let all: Vec<String> = playlist_track_ids(conn, id)?
        .into_iter()
        .filter(|t| !track_ids.contains(t))
        .collect();
    reorder_playlist(conn, id, &all, "playlist-remove")
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Library health
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// "identical-file": same content hash; "same-artist-title": same
    /// normalised artist + title (incl. version text) on different files.
    pub kind: String,
    pub key: String,
    pub tracks: Vec<TrackRow>,
}

/// Duplicate candidates. Nothing is ever deleted automatically; the user
/// reviews each group.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeSuspect {
    pub track: TrackRow,
    /// Rule id from `analysis::bandwidth::transcode_suspicion`.
    pub reason: String,
    pub cutoff_hz: Option<f64>,
    pub codec: Option<String>,
}

/// Tracks whose measured bandwidth suggests a lower-quality lossy source.
pub fn transcode_suspects(conn: &Connection, limit: usize) -> Result<Vec<TranscodeSuspect>> {
    let rows: Vec<(String, String, Option<f64>, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT t.id, t.transcode, t.cutoff_hz,
                    (SELECT codec FROM files f WHERE f.track_id = t.id ORDER BY missing, added_at LIMIT 1)
             FROM tracks t WHERE t.transcode IS NOT NULL ORDER BY t.cutoff_hz LIMIT ?1",
        )?;
        stmt.query_map([limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?
    };
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let tracks = crate::query::rows_for_ids(conn, &ids)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, reason, cutoff_hz, codec)| {
            let track = tracks.iter().find(|t| t.id == id)?.clone();
            Some(TranscodeSuspect {
                track,
                reason,
                cutoff_hz,
                codec,
            })
        })
        .collect())
}

pub fn duplicate_groups(conn: &Connection, limit: usize) -> Result<Vec<DuplicateGroup>> {
    let mut groups = Vec::new();
    let mut collect = |kind: &str, sql: &str| -> Result<()> {
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<(String, String)> = stmt
            .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (key, ids) in rows {
            let ids: Vec<String> = ids.split(',').map(String::from).collect();
            groups.push(DuplicateGroup {
                kind: kind.into(),
                key,
                tracks: crate::query::rows_for_ids(conn, &ids)?,
            });
        }
        Ok(())
    };
    collect(
        "identical-file",
        "SELECT quick_hash, group_concat(DISTINCT track_id) FROM files WHERE quick_hash IS NOT NULL
         GROUP BY quick_hash HAVING count(DISTINCT track_id) > 1 LIMIT ?1",
    )?;
    collect(
        "same-artist-title",
        "SELECT lower(trim(artist)) || ' — ' || lower(trim(title)), group_concat(id) FROM tracks
         WHERE artist IS NOT NULL AND title IS NOT NULL
         GROUP BY lower(trim(artist)), lower(trim(title)) HAVING count(*) > 1 LIMIT ?1",
    )?;
    Ok(groups)
}

// ---------------------------------------------------------------------------
// Watch folders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolder {
    pub path: String,
    pub added_at: i64,
    pub last_scan: Option<i64>,
    pub exists: bool,
}

pub fn watch_folders(conn: &Connection) -> Result<Vec<WatchFolder>> {
    let mut stmt =
        conn.prepare("SELECT path, added_at, last_scan FROM watch_folders ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        let path: String = r.get(0)?;
        Ok(WatchFolder {
            exists: Path::new(&path).is_dir(),
            path,
            added_at: r.get(1)?,
            last_scan: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn add_watch_folder(conn: &Connection, path: &Path) -> Result<String> {
    if !path.is_dir() {
        return Err(Error::Invalid(format!("not a folder: {}", path.display())));
    }
    let key = path_key(path);
    conn.execute(
        "INSERT INTO watch_folders (path, added_at) VALUES (?1, ?2) ON CONFLICT(path) DO NOTHING",
        params![key, now_ms()],
    )?;
    Ok(key)
}

pub fn remove_watch_folder(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM watch_folders WHERE path = ?1", [path])?;
    Ok(())
}

pub fn mark_watch_scanned(conn: &Connection, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE watch_folders SET last_scan = ?1 WHERE path = ?2",
        params![now_ms(), path],
    )?;
    Ok(())
}
