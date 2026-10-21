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

// ---------------------------------------------------------------------------
// Recoverable moves.
//
// SQLite and the file system are two separate transactional resources: a SQL
// rollback cannot undo a rename. Every move therefore goes through a small
// journal stored next to the library database (`<db>.fileops/`):
//
//   1. the source is identified (size + full BLAKE3) and a journal entry is
//      written and synced *before* the disk is touched;
//   2. the file is moved (same volume: no-overwrite link/rename; other volume:
//      copy to a `.deepcuts-partial.*` file, sync, verify the full hash,
//      atomic rename, then remove the source);
//   3. the database row is updated inside the caller's transaction;
//   4. when that transaction commits the journal entries are removed; when it
//      fails, the moves are reversed (`settle(false)`).
//
// A crash between 2 and 4 leaves the journal: `recover` (run when the
// database is opened) puts each file back where the committed database says
// it is, and removes abandoned partial copies.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JournalEntry {
    file_id: String,
    src: String,
    dst: String,
    size: u64,
    hash: String,
    /// "prepared" (nothing moved yet) or "moved".
    stage: String,
    /// Temporary copy used for a cross-volume move.
    partial: Option<String>,
}

thread_local! {
    /// Journal files of moves done inside the current transaction.
    static PENDING: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Journal directory for a connection (next to the database file).
fn journal_dir(conn: &Connection) -> PathBuf {
    match conn.path().filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(format!("{p}.fileops")),
        None => std::env::temp_dir().join(format!(
            "deepcuts-fileops-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )),
    }
}

fn sync_dir(dir: &Path) {
    // Directory fsync makes the rename durable on Unix; not available on Windows.
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

fn write_journal(path: &Path, e: &JournalEntry) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec(e)?)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(d) = path.parent() {
        sync_dir(d);
    }
    Ok(())
}

fn read_journal(path: &Path) -> Option<JournalEntry> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Size and full BLAKE3 hash of a file.
pub fn file_hash(path: &Path) -> Result<(u64, String)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, h.finalize().to_hex().to_string()))
}

/// Does `path` hold exactly the expected content?
fn holds(path: &Path, expected: &(u64, String)) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == expected.0)
        && file_hash(path).is_ok_and(|h| &h == expected)
}

/// Move one file on disk. Never overwrites an existing file (other than the
/// same file, e.g. a case-only rename). When the source is already gone the
/// move only counts as done if the destination holds exactly `expected`
/// (size and full hash), so an interrupted move can be repeated safely but
/// a different file at the destination is never accepted.
pub fn move_on_disk(from: &Path, to: &Path, expected: Option<&(u64, String)>) -> Result<()> {
    if fault::hit("move", from) {
        return Err(Error::Invalid(format!(
            "injected failure moving {}",
            from.display()
        )));
    }
    if !from.exists() {
        if to.exists() && expected.is_some_and(|e| holds(to, e)) {
            return Ok(());
        }
        return Err(if to.exists() {
            Error::Invalid(format!(
                "{} is missing and {} holds a different file: nothing changed",
                from.display(),
                to.display()
            ))
        } else {
            Error::NotFound(format!("{}", from.display()))
        });
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
    // Same volume. A hard link fails if the destination exists, so a file
    // created there since the check is never replaced.
    if !same_file(from, to) && std::fs::hard_link(from, to).is_ok() {
        if let Err(e) = std::fs::remove_file(from) {
            let _ = std::fs::remove_file(to);
            return Err(e.into());
        }
        if let Some(d) = to.parent() {
            sync_dir(d);
        }
        return Ok(());
    }
    if (same_file(from, to) || !to.exists()) && std::fs::rename(from, to).is_ok() {
        if let Some(d) = to.parent() {
            sync_dir(d);
        }
        return Ok(());
    }
    copy_verified(from, to)?;
    std::fs::remove_file(from)?;
    Ok(())
}

/// Cross-volume move, first half: copy to a temporary file next to the
/// destination, sync it, compare the full hash with the source, then rename
/// it into place. A failure removes the partial copy.
fn copy_verified(from: &Path, to: &Path) -> Result<()> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let partial = dir.join(format!(".deepcuts-partial.{}", crate::util::new_id()));
    let result = (|| -> Result<()> {
        let want = file_hash(from)?;
        std::fs::copy(from, &partial)?;
        std::fs::File::open(&partial)?.sync_all()?;
        if file_hash(&partial)? != want {
            return Err(Error::Invalid(format!(
                "copy of {} could not be verified",
                from.display()
            )));
        }
        if to.exists() {
            return Err(Error::Invalid(format!(
                "refusing to overwrite {}",
                to.display()
            )));
        }
        std::fs::rename(&partial, to)?;
        sync_dir(dir);
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

/// Path of a file row (history state for `Kind::FilePath`).
pub(crate) fn file_path(conn: &Connection, file_id: &str) -> Result<String> {
    Ok(
        conn.query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| {
            r.get(0)
        })?,
    )
}

/// Move a file and update its row (used by apply, undo and redo), with a
/// journal entry so the move is reversed if the transaction fails or the
/// process stops before it commits.
pub(crate) fn set_file_path(conn: &Connection, file_id: &str, to: &str) -> Result<()> {
    let from = file_path(conn, file_id)?;
    if from == to {
        return Ok(());
    }
    let (src, dst) = (Path::new(&from), Path::new(to));
    // Identity of the file being moved. If the source is already gone (a
    // repeated undo/redo), the destination must hold the file the database
    // describes (size + quick hash).
    let expected = if src.exists() {
        file_hash(src)?
    } else {
        let (size, quick): (i64, Option<String>) = conn.query_row(
            "SELECT size, quick_hash FROM files WHERE id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let ok = dst.exists()
            && std::fs::metadata(dst).is_ok_and(|m| m.len() == size as u64)
            && quick.is_some_and(|q| library::quick_hash(dst).is_ok_and(|h| h == q));
        if !ok {
            return Err(Error::Invalid(format!(
                "{from} is missing and {to} is not the same file: nothing changed"
            )));
        }
        file_hash(dst)?
    };
    let dir = journal_dir(conn);
    std::fs::create_dir_all(&dir)?;
    let jpath = dir.join(format!("{}.json", crate::util::new_id()));
    let mut entry = JournalEntry {
        file_id: file_id.to_string(),
        src: from.clone(),
        dst: to.to_string(),
        size: expected.0,
        hash: expected.1.clone(),
        stage: "prepared".into(),
        partial: None,
    };
    write_journal(&jpath, &entry)?;
    if let Err(e) = move_on_disk(src, dst, Some(&expected)) {
        let _ = std::fs::remove_file(&jpath);
        return Err(e);
    }
    entry.stage = "moved".into();
    write_journal(&jpath, &entry)?;
    if fault::hit("crash-after-move", src) {
        // Simulated process stop: the journal stays, nothing is reversed.
        return Err(Error::Invalid("injected crash after move".into()));
    }
    let to_key = library::path_key(dst);
    let updated = conn.execute(
        "UPDATE files SET path = ?1, missing = 0, last_seen = ?2 WHERE id = ?3",
        params![to_key, crate::util::now_ms(), file_id],
    );
    if let Err(e) = updated {
        // Put the file back now: this item failed, the batch may go on.
        if move_on_disk(dst, src, Some(&expected)).is_ok() {
            let _ = std::fs::remove_file(&jpath);
        }
        return Err(e.into());
    }
    if conn.is_autocommit() {
        // Not inside a transaction: the update is already durable.
        let _ = std::fs::remove_file(&jpath);
    } else {
        PENDING.with(|p| p.borrow_mut().push(jpath));
    }
    Ok(())
}

/// Fault injection for recovery tests (no effect unless a test sets it).
#[doc(hidden)]
pub mod fault {
    use std::cell::RefCell;
    use std::path::Path;

    thread_local! {
        static POINT: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    }

    /// Fail at `point` ("move" or "crash-after-move") for source paths
    /// containing `path_part`.
    pub fn set(point: &str, path_part: &str) {
        POINT.with(|p| *p.borrow_mut() = Some((point.into(), path_part.into())));
    }

    pub fn clear() {
        POINT.with(|p| *p.borrow_mut() = None);
    }

    pub(crate) fn hit(point: &str, path: &Path) -> bool {
        POINT.with(|p| {
            p.borrow().as_ref().is_some_and(|(pt, part)| {
                pt == point && path.to_string_lossy().contains(part.as_str())
            })
        })
    }
}

/// Called by `util::atomic` when its transaction ends: on commit the journal
/// entries are dropped; on rollback the moves are reversed, newest first.
/// An entry whose move cannot be reversed stays for `recover`.
pub(crate) fn settle(committed: bool) {
    let pending: Vec<PathBuf> = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for j in pending.into_iter().rev() {
        if committed {
            let _ = std::fs::remove_file(&j);
            continue;
        }
        let Some(e) = read_journal(&j) else { continue };
        let expected = (e.size, e.hash.clone());
        if move_on_disk(Path::new(&e.dst), Path::new(&e.src), Some(&expected)).is_ok() {
            let _ = std::fs::remove_file(&j);
        } else {
            eprintln!(
                "file move {} -> {} could not be reversed; it will be checked at the next start",
                e.src, e.dst
            );
        }
    }
}

/// Finish or reverse moves interrupted by a crash, using the committed
/// database as the reference. Returns one message per entry handled.
pub fn recover(conn: &Connection) -> Result<Vec<String>> {
    let dir = journal_dir(conn);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut msgs = Vec::new();
    for ent in rd.flatten() {
        let jpath = ent.path();
        if jpath.extension().and_then(|x| x.to_str()) != Some("json") {
            let _ = std::fs::remove_file(&jpath);
            continue;
        }
        let Some(e) = read_journal(&jpath) else {
            let _ = std::fs::remove_file(&jpath);
            continue;
        };
        let expected = (e.size, e.hash.clone());
        let (src, dst) = (Path::new(&e.src), Path::new(&e.dst));
        if let Some(p) = &e.partial {
            let _ = std::fs::remove_file(p);
        }
        // Abandoned cross-volume copies next to the destination.
        if let Some(d) = dst.parent()
            && let Ok(rd) = std::fs::read_dir(d)
        {
            for f in rd.flatten() {
                if f.file_name()
                    .to_string_lossy()
                    .starts_with(".deepcuts-partial.")
                {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        let db_path: Option<String> = conn
            .query_row("SELECT path FROM files WHERE id = ?1", [&e.file_id], |r| {
                r.get(0)
            })
            .optional()?;
        let at = |p: &Path| library::path_key(p);
        let target = match db_path {
            Some(p) if p == at(dst) => dst,
            Some(p) if p == at(src) => src,
            _ => {
                msgs.push(format!(
                    "interrupted move of {} could not be matched to the library; left as is",
                    e.src
                ));
                let _ = std::fs::remove_file(&jpath);
                continue;
            }
        };
        let other = if target == dst { src } else { dst };
        let msg = if holds(target, &expected) {
            // The file is where the database says; a second full copy left
            // by an interrupted cross-volume move is removed.
            if other.exists() && holds(other, &expected) {
                std::fs::remove_file(other)?;
            }
            format!("checked interrupted move of {}", e.src)
        } else if holds(other, &expected) {
            move_on_disk(other, target, Some(&expected))?;
            format!(
                "completed interrupted move: {} is back at {}",
                e.src,
                target.display()
            )
        } else {
            format!(
                "interrupted move of {}: file not found at either path; left for review",
                e.src
            )
        };
        msgs.push(msg);
        let _ = std::fs::remove_file(&jpath);
    }
    Ok(msgs)
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
            energy: None,
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
