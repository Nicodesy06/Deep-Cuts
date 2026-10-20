//! Merging duplicates without losing anything.
//!
//! One track is kept. For every other copy, in one undoable batch:
//! playlist entries are re-pointed to the kept track, its decision, user
//! tags, favourite, rating and notes are carried over where the kept track
//! has none, its files are moved to a trash folder (never deleted), and the
//! copy is archived (hidden, with its cues and history intact). Undo moves
//! the files back and restores every playlist.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::library;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub merged: usize,
    pub playlists_updated: usize,
    pub files_moved: usize,
    pub trash_dir: String,
    pub failed: Vec<String>,
}

/// Quality rank used to suggest which copy to keep: present file, not a
/// suspected transcode, lossless, then higher bitrate.
pub fn suggest_keep(conn: &Connection, ids: &[String]) -> Result<Option<String>> {
    let mut best: Option<(String, (bool, bool, bool, i64))> = None;
    for id in ids {
        let r: Option<(bool, bool, Option<String>, Option<i64>)> = conn
            .query_row(
                "SELECT COALESCE(f.missing, 1), t.transcode IS NOT NULL, f.codec, f.bitrate
                 FROM tracks t LEFT JOIN files f ON f.id = (SELECT id FROM files WHERE track_id = t.id ORDER BY missing, added_at LIMIT 1)
                 WHERE t.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((missing, transcode, codec, bitrate)) = r else {
            continue;
        };
        let lossless = codec
            .as_deref()
            .is_some_and(crate::audio::is_lossless_codec);
        let rank = (!missing, !transcode, lossless, bitrate.unwrap_or(0));
        if best.as_ref().is_none_or(|(_, b)| rank > *b) {
            best = Some((id.clone(), rank));
        }
    }
    Ok(best.map(|b| b.0))
}

/// A path in `dir` for `name` that does not exist yet.
fn free_path(dir: &Path, name: &str, taken: &[PathBuf]) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(format!("{stem}{ext}"))
        } else {
            dir.join(format!("{stem} ({n}){ext}"))
        };
        if !candidate.exists() && !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

pub fn merge(
    conn: &Connection,
    keep: &str,
    others: &[String],
    trash: &Path,
) -> Result<MergeReport> {
    let others: Vec<String> = others
        .iter()
        .filter(|o| o.as_str() != keep)
        .cloned()
        .collect();
    if others.is_empty() {
        return Err(Error::Invalid("nothing to merge".into()));
    }
    library::get_track(conn, keep)?;
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let trash_dir = trash.join(day);
    let mut rep = MergeReport {
        trash_dir: trash_dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let summary = format!("{} copies into 1", others.len());
    history::batch(conn, "merge-duplicates", &summary, |tx, b| {
        // Playlists: replace each copy by the kept track (once per playlist).
        let pids: Vec<String> = {
            let marks = vec!["?"; others.len()].join(",");
            let mut stmt = tx.prepare(&format!(
                "SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id IN ({marks})"
            ))?;
            stmt.query_map(rusqlite::params_from_iter(others.iter()), |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        for pid in &pids {
            let mut seen_keep = false;
            let mut out: Vec<String> = Vec::new();
            for t in library::playlist_track_ids(tx, pid)? {
                let t = if others.contains(&t) {
                    keep.to_string()
                } else {
                    t
                };
                if t == keep {
                    if seen_keep {
                        continue;
                    }
                    seen_keep = true;
                }
                out.push(t);
            }
            history::change(
                tx,
                b,
                Kind::PlaylistOrder,
                pid,
                pid,
                State::set(Some(&serde_json::to_string(&out)?)),
            )?;
            rep.playlists_updated += 1;
        }
        let mut taken: Vec<PathBuf> = Vec::new();
        for other in &others {
            // Decision, favourite, notes, rating: only where the kept track has none.
            let dec: Option<String> = tx
                .query_row(
                    "SELECT decision FROM decisions WHERE track_id = ?1 AND context = 'global'",
                    [other],
                    |r| r.get(0),
                )
                .optional()?;
            let keep_dec: Option<String> = tx
                .query_row(
                    "SELECT decision FROM decisions WHERE track_id = ?1 AND context = 'global'",
                    [keep],
                    |r| r.get(0),
                )
                .optional()?;
            if let (Some(d), None) = (&dec, &keep_dec) {
                history::change(tx, b, Kind::Decision, keep, "global", State::set(Some(d)))?;
            }
            let (fav, notes, rating): (bool, Option<String>, Option<i64>) = tx.query_row(
                "SELECT favorite, notes, rating FROM tracks WHERE id = ?1",
                [other],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let (kfav, knotes, krating): (bool, Option<String>, Option<i64>) = tx.query_row(
                "SELECT favorite, notes, rating FROM tracks WHERE id = ?1",
                [keep],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            if fav && !kfav {
                history::change(
                    tx,
                    b,
                    Kind::Favorite,
                    keep,
                    "favorite",
                    State::set(Some("1")),
                )?;
            }
            if notes.as_deref().is_some_and(|n| !n.is_empty())
                && knotes.as_deref().is_none_or(str::is_empty)
            {
                history::change(
                    tx,
                    b,
                    Kind::Notes,
                    keep,
                    "notes",
                    State::set(notes.as_deref()),
                )?;
            }
            if rating.unwrap_or(0) > 0 && krating.unwrap_or(0) == 0 {
                history::change(
                    tx,
                    b,
                    Kind::Field,
                    keep,
                    "rating",
                    State::set(Some(&rating.unwrap().to_string())),
                )?;
            }
            // User tags.
            let tags: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT tag_id FROM track_tags WHERE track_id = ?1 AND origin = 'user' AND status = 'confirmed'",
                )?;
                stmt.query_map([other], |r| r.get(0))?
                    .collect::<std::result::Result<_, _>>()?
            };
            for tag in tags {
                history::change(tx, b, Kind::Tag, keep, &tag, State::set(None))?;
            }
            // Files → trash (moved, verified, never deleted).
            let files: Vec<(String, String, bool)> = {
                let mut stmt =
                    tx.prepare("SELECT id, path, missing FROM files WHERE track_id = ?1")?;
                stmt.query_map([other], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<std::result::Result<_, _>>()?
            };
            for (fid, path, missing) in files {
                let src = PathBuf::from(&path);
                if missing || !src.exists() {
                    continue;
                }
                std::fs::create_dir_all(&trash_dir)?;
                let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("track");
                let dst = free_path(&trash_dir, name, &taken);
                taken.push(dst.clone());
                match history::change(
                    tx,
                    b,
                    Kind::FilePath,
                    other,
                    &fid,
                    State::set(Some(&dst.to_string_lossy())),
                ) {
                    Ok(_) => rep.files_moved += 1,
                    Err(e) => rep.failed.push(format!("{path}: {e}")),
                }
            }
            history::change(
                tx,
                b,
                Kind::Archived,
                other,
                "archived",
                State::set(Some("1")),
            )?;
            rep.merged += 1;
        }
        library::refresh_fts(tx, keep)?;
        Ok(())
    })?;
    Ok(rep)
}

/// Playlists that reference tracks whose files are all missing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenPlaylist {
    pub id: String,
    pub name: String,
    pub missing: i64,
    pub total: i64,
}

pub fn broken_playlists(conn: &Connection) -> Result<Vec<BrokenPlaylist>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name,
                sum(NOT EXISTS (SELECT 1 FROM files f WHERE f.track_id = pt.track_id AND f.missing = 0)),
                count(*)
         FROM playlists p JOIN playlist_tracks pt ON pt.playlist_id = p.id
         GROUP BY p.id HAVING sum(NOT EXISTS (SELECT 1 FROM files f WHERE f.track_id = pt.track_id AND f.missing = 0)) > 0
         ORDER BY p.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(BrokenPlaylist {
            id: r.get(0)?,
            name: r.get(1)?,
            missing: r.get(2)?,
            total: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Audio files under `root` that are not in the library (read-only scan).
pub fn unused_files(conn: &Connection, root: &Path) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT 1 FROM files WHERE path = ?1")?;
    let mut out = Vec::new();
    for f in library::collect_audio_files(&[root.to_path_buf()]) {
        let key = library::path_key(&f);
        if !stmt.exists([&key])? {
            out.push(key);
        }
    }
    Ok(out)
}
