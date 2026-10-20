//! Rename and move audio files from metadata, with preview and undo.
//!
//! A plan is computed first (nothing touches the disk) and shows conflicts.
//! Applying a plan moves the files and records every move in the undo
//! history (`history::Kind::FilePath`), so undo moves them back. Existing
//! files are never overwritten. Moves across volumes copy, verify the copy
//! by size and content hash, and only then delete the original.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use crate::library::{self, TrackRow};
use crate::query::rows_for_ids;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const DEFAULT_PATTERN: &str = "{artist} - {title} ({version})";

pub const TOKENS: &[&str] = &[
    "artist", "title", "version", "remixer", "album", "label", "genre", "year", "bpm", "key",
    "camelot",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MovePlan {
    pub track_id: String,
    pub file_id: String,
    pub from: String,
    pub to: String,
    /// Why this file is skipped by `apply`: no-metadata, missing,
    /// unchanged, duplicate-in-plan, exists or no-target.
    pub conflict: Option<String>,
}

/// Characters that are invalid in file names on Windows, macOS or Linux.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn token_value(t: &TrackRow, token: &str) -> Option<String> {
    let s = match token {
        "artist" => t.artist.clone(),
        "title" => t.title.clone(),
        "version" => t.version.clone(),
        "remixer" => t.remixer.clone(),
        "album" => t.album.clone(),
        "label" => t.label.clone(),
        "genre" => t.genre.clone(),
        "year" => t.year.map(|y| y.to_string()),
        "bpm" => t.bpm.map(|b| format!("{}", b.round() as i64)),
        "key" => t.key.clone(),
        "camelot" => t.camelot.clone(),
        _ => None,
    }?;
    let s = sanitize(&s);
    (!s.is_empty()).then_some(s)
}

/// Fill a pattern such as `{artist} - {title} ({version})`. Brackets and
/// separators around empty tokens are dropped. Returns None if no token
/// produced a value.
pub fn render(pattern: &str, t: &TrackRow) -> Result<Option<String>> {
    let mut out = String::new();
    let mut any = false;
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..]
            .find('}')
            .ok_or_else(|| Error::Invalid("unclosed '{' in pattern".into()))?
            + start;
        let token = &rest[start + 1..end];
        if !TOKENS.contains(&token) {
            return Err(Error::Invalid(format!("unknown token {{{token}}}")));
        }
        // Titles often already carry the version ("Track (Extended Mix)").
        let value = token_value(t, token).filter(|v| {
            !(token == "version" || token == "remixer")
                || !t
                    .title
                    .as_deref()
                    .is_some_and(|title| title.to_lowercase().contains(&v.to_lowercase()))
        });
        match value {
            Some(v) => {
                out.push_str(&v);
                any = true;
            }
            None => out.push('\u{0}'),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    if !any {
        return Ok(None);
    }
    // Remove empty brackets and dangling separators left by empty tokens.
    let mut s = out;
    for pair in ["(\u{0})", "[\u{0}]", "{\u{0}}"] {
        s = s.replace(pair, "");
    }
    s = s.replace('\u{0}', "");
    for empty in ["()", "[]"] {
        s = s.replace(empty, "");
    }
    let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let t = s
            .trim_matches(|c: char| c == '-' || c == ',' || c.is_whitespace())
            .to_string();
        let t = t.replace(" - - ", " - ");
        if t == s {
            break;
        }
        s = t;
    }
    // Windows does not allow trailing dots or spaces.
    let s = s.trim_end_matches(['.', ' ']).to_string();
    Ok((!s.is_empty()).then_some(s))
}

fn primary_file(conn: &Connection, track_id: &str) -> Result<Option<(String, String, bool)>> {
    Ok(conn
        .query_row(
            "SELECT id, path, missing FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Compute the moves for `ids` (dry run). `target_dir` = None keeps each
/// file in its current folder.
pub fn plan(
    conn: &Connection,
    ids: &[String],
    pattern: &str,
    target_dir: Option<&Path>,
) -> Result<Vec<MovePlan>> {
    let mut taken: HashSet<String> = HashSet::new();
    let mut plans = Vec::new();
    for t in rows_for_ids(conn, ids)? {
        let Some((file_id, from, missing)) = primary_file(conn, &t.id)? else {
            continue;
        };
        let from_p = PathBuf::from(&from);
        let ext = from_p
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let dir = target_dir
            .map(Path::to_path_buf)
            .or_else(|| from_p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let mut conflict = None;
        let to = match render(pattern, &t)? {
            Some(stem) => dir.join(format!("{stem}{ext}")),
            None => {
                conflict = Some("no-metadata".into());
                from_p.clone()
            }
        };
        let to_s = to.to_string_lossy().into_owned();
        if conflict.is_none() {
            if missing || !from_p.exists() {
                conflict = Some("missing".into());
            } else if to_s == from {
                conflict = Some("unchanged".into());
            } else if taken.contains(&to_s.to_lowercase()) {
                conflict = Some("duplicate-in-plan".into());
            } else if to.exists() && !same_file(&from_p, &to) {
                conflict = Some("exists".into());
            } else if target_dir.is_some_and(|d| !d.is_dir()) {
                conflict = Some("no-target".into());
            }
        }
        if conflict.is_none() {
            taken.insert(to_s.to_lowercase());
        }
        plans.push(MovePlan {
            track_id: t.id,
            file_id,
            from,
            to: to_s,
            conflict,
        });
    }
    Ok(plans)
}

/// True when two paths name the same file (e.g. a case-only rename on a
/// case-insensitive file system).
fn same_file(a: &Path, b: &Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Move one file on disk. Never overwrites a different existing file.
/// Idempotent: if the file is already at `to` (and gone from `from`), it is
/// a no-op, so an interrupted undo/redo can simply be repeated.
pub fn move_on_disk(from: &Path, to: &Path) -> Result<()> {
    let from_exists = from.exists();
    if !from_exists && to.exists() {
        return Ok(());
    }
    if !from_exists {
        return Err(Error::NotFound(format!("{}", from.display())));
    }
    if to.exists() && !same_file(from, to) {
        return Err(Error::Invalid(format!(
            "refusing to overwrite {}",
            to.display()
        )));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Different volume: copy, verify, then remove the original.
    std::fs::copy(from, to)?;
    let ok = std::fs::metadata(from)?.len() == std::fs::metadata(to)?.len()
        && library::quick_hash(from)? == library::quick_hash(to)?;
    if !ok {
        let _ = std::fs::remove_file(to);
        return Err(Error::Invalid(format!(
            "copy of {} could not be verified",
            from.display()
        )));
    }
    std::fs::remove_file(from)?;
    Ok(())
}

/// Path of a file row (history state for `Kind::FilePath`).
pub(crate) fn file_path(conn: &Connection, file_id: &str) -> Result<String> {
    Ok(
        conn.query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| {
            r.get(0)
        })?,
    )
}

/// Move a file and update its row (used by apply, undo and redo).
pub(crate) fn set_file_path(conn: &Connection, file_id: &str, to: &str) -> Result<()> {
    let from = file_path(conn, file_id)?;
    if from == to {
        return Ok(());
    }
    move_on_disk(Path::new(&from), Path::new(to))?;
    let to_key = library::path_key(Path::new(to));
    conn.execute(
        "UPDATE files SET path = ?1, missing = 0, last_seen = ?2 WHERE id = ?3",
        params![to_key, crate::util::now_ms(), file_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, version: Option<&str>) -> TrackRow {
        TrackRow {
            id: "x".into(),
            title: Some(title.into()),
            artist: Some("AC/DC".into()),
            version: version.map(String::from),
            remixer: None,
            album: None,
            label: None,
            genre: None,
            year: None,
            bpm: Some(124.4),
            key: None,
            camelot: Some("8A".into()),
            duration_ms: None,
            rating: None,
            favorite: false,
            decision: None,
            comment: None,
            path: None,
            format: None,
            bitrate: None,
            missing: false,
            lufs: None,
            preview_ms: None,
            analysis_status: "none".into(),
            created_at: 0,
        }
    }

    #[test]
    fn render_patterns() {
        let p = "{artist} - {title} ({version}) [{camelot}] {bpm}";
        assert_eq!(
            render(p, &row("Thunder", Some("Extended Mix")))
                .unwrap()
                .unwrap(),
            "AC_DC - Thunder (Extended Mix) [8A] 124"
        );
        assert_eq!(
            render(p, &row("Thunder", None)).unwrap().unwrap(),
            "AC_DC - Thunder [8A] 124"
        );
        // version already in the title is not repeated
        assert_eq!(
            render(
                DEFAULT_PATTERN,
                &row("Thunder (Extended Mix)", Some("Extended Mix"))
            )
            .unwrap()
            .unwrap(),
            "AC_DC - Thunder (Extended Mix)"
        );
        let mut empty = row("", None);
        empty.title = None;
        empty.artist = None;
        assert_eq!(render("{artist} - {title}", &empty).unwrap(), None);
        assert!(render("{bogus}", &empty).is_err());
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub moved: usize,
    pub skipped: usize,
    pub failed: Vec<MoveFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveFailure {
    pub path: String,
    pub error: String,
}

/// Apply a plan as one undoable batch. Entries with a conflict are skipped.
/// A file that fails to move is reported and left where it was; the others
/// are still moved.
pub fn apply(conn: &Connection, plans: &[MovePlan]) -> Result<ApplyReport> {
    let mut rep = ApplyReport::default();
    let todo: Vec<&MovePlan> = plans.iter().filter(|p| p.conflict.is_none()).collect();
    rep.skipped = plans.len() - todo.len();
    history::batch(
        conn,
        "rename-files",
        &format!("{} files", todo.len()),
        |tx, b| {
            for p in todo {
                // The plan may be stale: re-check the source.
                if file_path(tx, &p.file_id)? != p.from {
                    rep.failed.push(MoveFailure {
                        path: p.from.clone(),
                        error: "file changed since the preview".into(),
                    });
                    continue;
                }
                match history::change(
                    tx,
                    b,
                    Kind::FilePath,
                    &p.track_id,
                    &p.file_id,
                    State::set(Some(&p.to)),
                ) {
                    Ok(_) => rep.moved += 1,
                    Err(e) => rep.failed.push(MoveFailure {
                        path: p.from.clone(),
                        error: e.to_string(),
                    }),
                }
            }
            Ok(())
        },
    )?;
    Ok(rep)
}
