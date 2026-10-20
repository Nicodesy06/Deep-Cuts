//! Relocate the library: rewrite file paths that start with one folder to
//! another (new drive letter, external disk mounted elsewhere, library copied
//! to another computer). Only the database changes; files are not moved.
//! Undoable.

use crate::error::{Error, Result};
use crate::history::{self, Kind, State};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocatePreview {
    /// Files whose path starts with the old folder.
    pub matched: usize,
    /// How many of them exist at the new location.
    pub found: usize,
    /// A few examples (old, new, exists).
    pub samples: Vec<(String, String, bool)>,
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// New path for `path` if it lies under `from`, using `to`'s separator style.
pub fn map_path(path: &str, from: &str, to: &str) -> Option<String> {
    let from = from.trim_end_matches(is_sep);
    if from.is_empty() || !path.starts_with(from) {
        return None;
    }
    let rest = &path[from.len()..];
    if !rest.is_empty() && !rest.starts_with(is_sep) {
        return None; // "/Music2" is not under "/Music"
    }
    let sep = if to.contains('\\') && !to.contains('/') {
        '\\'
    } else {
        '/'
    };
    let rest: String = rest
        .trim_start_matches(is_sep)
        .chars()
        .map(|c| if is_sep(c) { sep } else { c })
        .collect();
    let to = to.trim_end_matches(is_sep);
    Some(if rest.is_empty() {
        to.to_string()
    } else {
        format!("{to}{sep}{rest}")
    })
}

fn candidates(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn.prepare("SELECT id, track_id, path FROM files")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (fid, tid, path) = r?;
        if let Some(new) = map_path(&path, from, to) {
            out.push((fid, tid, path, new));
        }
    }
    Ok(out)
}

pub fn preview(conn: &Connection, from: &str, to: &str) -> Result<RelocatePreview> {
    let c = candidates(conn, from, to)?;
    let found = c.iter().filter(|x| Path::new(&x.3).exists()).count();
    Ok(RelocatePreview {
        matched: c.len(),
        found,
        samples: c
            .iter()
            .take(8)
            .map(|x| (x.2.clone(), x.3.clone(), Path::new(&x.3).exists()))
            .collect(),
    })
}

/// Rewrite the paths; with `only_found`, only files that exist at the new
/// location are changed. Returns the number of files changed.
pub fn apply(conn: &Connection, from: &str, to: &str, only_found: bool) -> Result<usize> {
    if from.trim_end_matches(is_sep).is_empty() {
        return Err(Error::Invalid("the old folder is empty".into()));
    }
    let c = candidates(conn, from, to)?;
    history::batch(conn, "relocate", &format!("{from} → {to}"), |tx, b| {
        let mut n = 0;
        for (fid, tid, _, new) in c {
            if only_found && !Path::new(&new).exists() {
                continue;
            }
            // Never collide with a file the library already knows.
            if tx
                .prepare_cached("SELECT 1 FROM files WHERE path = ?1")?
                .exists([&new])?
            {
                continue;
            }
            if history::change(tx, b, Kind::FileRecord, &tid, &fid, State::set(Some(&new)))? {
                n += 1;
            }
        }
        Ok(n)
    })
}

/// Folders that contain missing files, with how many (most first).
pub fn missing_folders(conn: &Connection, limit: usize) -> Result<Vec<(String, i64)>> {
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut stmt = conn.prepare("SELECT path FROM files WHERE missing = 1")?;
    for p in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let p = p?;
        if let Some(i) = p.rfind(is_sep) {
            *counts.entry(p[..i].to_string()).or_default() += 1;
        }
    }
    let mut v: Vec<(String, i64)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(limit);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::map_path;

    #[test]
    fn prefix_mapping() {
        assert_eq!(
            map_path("D:\\Music\\House\\a.mp3", "D:\\Music", "/Volumes/USB/Music").as_deref(),
            Some("/Volumes/USB/Music/House/a.mp3")
        );
        assert_eq!(
            map_path("/Users/a/Music/x.flac", "/Users/a/Music/", "E:\\Music").as_deref(),
            Some("E:\\Music\\x.flac")
        );
        assert_eq!(map_path("/Music2/x.flac", "/Music", "/New"), None);
        assert_eq!(map_path("/Other/x.flac", "/Music", "/New"), None);
    }
}
