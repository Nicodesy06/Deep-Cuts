//! Writing metadata back into audio files — the only place Deep Cuts
//! modifies the user's audio files, and only on explicit request.
//!
//! Safety: plan first (dry run), refuse files that changed on disk since
//! they were scanned, optionally copy the original before writing, verify
//! by re-reading, and restore the copy if verification fails.

use crate::error::{Error, Result};
use crate::library::{self, SOURCE_FILE};
use crate::metadata;
use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Fields that can be written to tags.
pub const WRITABLE: &[&str] = &[
    "title",
    "artist",
    "album",
    "albumArtist",
    "label",
    "genre",
    "remixer",
    "year",
    "bpm",
    "key",
    "comment",
    "isrc",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    pub file_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WritePlan {
    pub track_id: String,
    pub path: String,
    pub changes: Vec<FieldChange>,
}

fn norm(field: &str, v: Option<String>) -> Option<String> {
    let v = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())?;
    Some(match field {
        "bpm" => v
            .parse::<f64>()
            .map(|b| {
                format!("{:.2}", b)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            })
            .unwrap_or(v),
        "year" => v
            .parse::<f64>()
            .map(|y| (y as i64).to_string())
            .unwrap_or(v),
        _ => v,
    })
}

/// (field, value in the file, effective library value)
type FieldPair = (String, Option<String>, Option<String>);

fn effective_values(conn: &Connection, track_id: &str) -> Result<Vec<FieldPair>> {
    // (field, file value, effective value) for writable fields.
    let rank = |s: &str| match s {
        "user" => 0,
        s if s.starts_with("import:") => 1,
        "file" => 2,
        _ => 3,
    };
    let fields = library::field_values(conn, track_id)?;
    let mut out = Vec::new();
    for &f in WRITABLE {
        let mut vals: Vec<_> = fields.iter().filter(|v| v.field == f).collect();
        vals.sort_by_key(|v| rank(&v.source));
        let file = vals
            .iter()
            .find(|v| v.source == SOURCE_FILE)
            .and_then(|v| v.value.clone());
        // Analysis values are written only for BPM and key (DJ software reads them).
        let eff = vals
            .iter()
            .find(|v| v.source != "analysis" || f == "bpm" || f == "key")
            .and_then(|v| v.value.clone());
        out.push((f.to_string(), norm(f, file), norm(f, eff)));
    }
    Ok(out)
}

/// ID3v2 (MP3, AIFF, WAV) and MP4 only have an integer BPM frame: a file
/// holding the rounded value is in sync with a decimal library BPM.
fn bpm_in_sync(file: &Option<String>, eff: &Option<String>) -> bool {
    match (
        file.as_deref().and_then(|f| f.parse::<f64>().ok()),
        eff.as_deref().and_then(|e| e.parse::<f64>().ok()),
    ) {
        (Some(f), Some(e)) => f.fract() == 0.0 && f == e.round(),
        _ => false,
    }
}

/// Dry run: what would change in each file.
pub fn plan(conn: &Connection, ids: &[String]) -> Result<Vec<WritePlan>> {
    let mut out = Vec::new();
    for id in ids {
        let path = match library::track_path(conn, id) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let changes: Vec<FieldChange> = effective_values(conn, id)?
            .into_iter()
            .filter(|(field, file, eff)| file != eff && !(field == "bpm" && bpm_in_sync(file, eff)))
            .map(|(field, file_value, new_value)| FieldChange {
                field,
                file_value,
                new_value,
            })
            .collect();
        if !changes.is_empty() {
            out.push(WritePlan {
                track_id: id.clone(),
                path: path.to_string_lossy().into_owned(),
                changes,
            });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WriteReport {
    pub written: usize,
    pub unchanged: usize,
    pub failed: Vec<WriteFailure>,
    pub backup_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFailure {
    pub track_id: String,
    pub path: String,
    pub error: String,
}

fn item_for(field: &str) -> Option<ItemKey> {
    Some(match field {
        "albumArtist" => ItemKey::AlbumArtist,
        "label" => ItemKey::Label,
        "remixer" => ItemKey::Remixer,
        "key" => ItemKey::InitialKey,
        "isrc" => ItemKey::Isrc,
        _ => return None,
    })
}

fn apply_to_tag(tag: &mut Tag, changes: &[FieldChange]) {
    for c in changes {
        let v = c.new_value.clone();
        match c.field.as_str() {
            "title" => match v {
                Some(v) => tag.set_title(v),
                None => tag.remove_title(),
            },
            "artist" => match v {
                Some(v) => tag.set_artist(v),
                None => tag.remove_artist(),
            },
            "album" => match v {
                Some(v) => tag.set_album(v),
                None => tag.remove_album(),
            },
            "genre" => match v {
                Some(v) => tag.set_genre(v),
                None => tag.remove_genre(),
            },
            "comment" => match v {
                Some(v) => tag.set_comment(v),
                None => tag.remove_comment(),
            },
            "year" => match v.and_then(|y| y.parse::<u16>().ok()) {
                Some(y) => tag.set_date(lofty::tag::items::Timestamp {
                    year: y,
                    ..Default::default()
                }),
                None => tag.remove_date(),
            },
            "bpm" => {
                tag.remove_key(ItemKey::Bpm);
                tag.remove_key(ItemKey::IntegerBpm);
                if let Some(b) = v.and_then(|b| b.parse::<f64>().ok()) {
                    // Integer frame (TBPM / tmpo) for compatibility, exact value where the format allows.
                    tag.insert_text(ItemKey::IntegerBpm, format!("{}", b.round() as i64));
                    if b.fract().abs() > 1e-9 {
                        tag.insert_text(ItemKey::Bpm, format!("{b}"));
                    }
                }
            }
            f => {
                if let Some(key) = item_for(f) {
                    tag.remove_key(key);
                    if let Some(v) = v {
                        tag.insert_text(key, v);
                    }
                }
            }
        }
    }
}

fn verify(path: &Path, changes: &[FieldChange]) -> Result<()> {
    let m = metadata::read(path)?;
    for c in changes {
        let got = match c.field.as_str() {
            "title" => m.title.clone(),
            "artist" => m.artist.clone(),
            "album" => m.album.clone(),
            "albumArtist" => m.album_artist.clone(),
            "label" => m.label.clone(),
            "genre" => m.genre.clone(),
            "remixer" => m.remixer.clone(),
            "year" => m.year.map(|y| y.to_string()),
            "bpm" => m.bpm.map(|b| b.to_string()),
            "key" => m.musical_key.clone(),
            "comment" => m.comment.clone(),
            "isrc" => m.isrc.clone(),
            _ => continue,
        };
        let want = c.new_value.clone();
        let ok = match c.field.as_str() {
            // Formats without a decimal BPM frame keep the rounded value.
            "bpm" => match (
                got.as_deref().and_then(|g| g.parse::<f64>().ok()),
                want.as_deref().and_then(|w| w.parse::<f64>().ok()),
            ) {
                (Some(g), Some(w)) => (g - w).abs() <= 0.5,
                (None, None) => true,
                _ => false,
            },
            // Title/remixer may legitimately be re-derived from the file name when removed.
            "title" | "remixer" if want.is_none() => true,
            _ => norm(&c.field, got.clone()) == want,
        };
        if !ok {
            return Err(Error::Metadata(format!(
                "verification failed for {}: wrote {:?}, read back {:?}",
                c.field, want, got
            )));
        }
    }
    Ok(())
}

/// Write planned changes to the files. `backup_dir`: copy each original
/// there first (recommended). Files that changed on disk since the last
/// scan are skipped.
pub fn write(
    conn: &mut Connection,
    ids: &[String],
    backup_dir: Option<&Path>,
) -> Result<WriteReport> {
    let plans = plan(conn, ids)?;
    let mut rep = WriteReport {
        unchanged: ids.len().saturating_sub(plans.len()),
        ..Default::default()
    };
    let backup_root: Option<PathBuf> =
        backup_dir.map(|d| d.join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()));
    if let Some(b) = &backup_root {
        std::fs::create_dir_all(b)?;
        rep.backup_dir = Some(b.to_string_lossy().into_owned());
    }
    for p in plans {
        let path = PathBuf::from(&p.path);
        let fail = |e: String| WriteFailure {
            track_id: p.track_id.clone(),
            path: p.path.clone(),
            error: e,
        };
        let known: Option<(String, i64, Option<i64>)> = conn
            .query_row(
                "SELECT id, size, mtime_ms FROM files WHERE path = ?1",
                [&p.path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((file_id, size, mtime)) = known else {
            rep.failed.push(fail("file is not in the library".into()));
            continue;
        };
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                rep.failed.push(fail(format!("cannot access file: {e}")));
                continue;
            }
        };
        let disk_mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        if meta.len() as i64 != size || disk_mtime != mtime {
            rep.failed.push(fail(
                "file changed on disk since it was scanned: re-import it first".into(),
            ));
            continue;
        }
        if meta.permissions().readonly() {
            rep.failed.push(fail("file is read-only".into()));
            continue;
        }
        let copy = match &backup_root {
            Some(root) => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
                let dst = root.join(format!("{}.{ext}", p.track_id));
                if let Err(e) = std::fs::copy(&path, &dst) {
                    rep.failed
                        .push(fail(format!("backup copy failed, nothing written: {e}")));
                    continue;
                }
                Some(dst)
            }
            None => None,
        };
        let result = (|| -> Result<()> {
            let mut tagged =
                lofty::read_from_path(&path).map_err(|e| Error::Metadata(e.to_string()))?;
            let tag_type = tagged.primary_tag_type();
            if tagged.tag_mut(tag_type).is_none() {
                tagged.insert_tag(Tag::new(tag_type));
            }
            let tag = tagged.tag_mut(tag_type).expect("tag inserted above");
            apply_to_tag(tag, &p.changes);
            tagged
                .save_to_path(&path, WriteOptions::default())
                .map_err(|e| Error::Metadata(e.to_string()))?;
            verify(&path, &p.changes)
        })();
        match result {
            Ok(()) => {
                // The file now carries these values: refresh the file source.
                let m = metadata::read(&path)?;
                let new_meta = std::fs::metadata(&path)?;
                let hash = library::quick_hash(&path)?;
                let mtime = new_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64);
                crate::util::atomic(conn, |tx| {
                    tx.execute(
                        "UPDATE files SET size = ?1, mtime_ms = ?2, quick_hash = ?3 WHERE id = ?4",
                        params![new_meta.len() as i64, mtime, hash, file_id],
                    )?;
                    library::apply_file_metadata(tx, &p.track_id, &m)?;
                    library::refresh_fts(tx, &p.track_id)?;
                    library::audit(
                        tx,
                        "write-tags",
                        "file",
                        Some(&file_id),
                        serde_json::json!({ "path": p.path, "changes": p.changes, "backup": copy }),
                    )
                })?;
                rep.written += 1;
            }
            Err(e) => {
                let mut msg = e.to_string();
                if let Some(c) = &copy {
                    match std::fs::copy(c, &path) {
                        Ok(_) => msg.push_str(" (original restored)"),
                        Err(re) => msg.push_str(&format!(
                            " (RESTORE FAILED: {re}; original kept at {})",
                            c.display()
                        )),
                    }
                }
                rep.failed.push(fail(msg));
            }
        }
    }
    Ok(rep)
}
