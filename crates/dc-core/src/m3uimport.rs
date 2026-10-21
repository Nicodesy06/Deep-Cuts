//! Import an M3U/M3U8 playlist file (e.g. exported from Rekordbox) as a
//! playlist: files not yet in the library are imported first (read-only, so
//! their comments' My Tags are read too, see `mytag`), then a playlist with
//! the same order is created. Entries whose file does not exist are listed.

use crate::error::{Error, Result};
use crate::library;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3uImport {
    pub playlist_id: Option<String>,
    pub name: String,
    pub entries: usize,
    /// Entries that were already in the library.
    pub in_library: usize,
    /// Files imported into the library by this import.
    pub imported: usize,
    /// Entries whose file was not found.
    pub missing: Vec<String>,
    pub my_tags: Option<crate::mytag::MyTagReport>,
    /// Tracks added to the library by this import (to queue for analysis).
    pub new_track_ids: Vec<String>,
}

/// File paths listed in a playlist, resolved against the playlist's folder.
pub fn entries(text: &str, base: &Path) -> Vec<PathBuf> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let l = match l.strip_prefix("file://") {
                Some(rest) => {
                    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                    // file:///C:/x → C:/x on Windows paths.
                    let rest = match rest.as_bytes() {
                        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => &rest[1..],
                        _ => rest,
                    };
                    percent_encoding::percent_decode_str(rest)
                        .decode_utf8_lossy()
                        .into_owned()
                }
                None => l.to_string(),
            };
            let p = PathBuf::from(&l);
            if p.is_absolute() || l.starts_with('\\') || l.get(1..2) == Some(":") {
                p
            } else {
                base.join(p)
            }
        })
        .collect()
}

fn track_for(conn: &Connection, path: &Path) -> Result<Option<String>> {
    let key = library::path_key(path);
    Ok(conn
        .query_row("SELECT track_id FROM files WHERE path = ?1", [&key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn import(conn: &mut Connection, file: &Path) -> Result<M3uImport> {
    let bytes = std::fs::read(file)?;
    let text = String::from_utf8_lossy(&bytes);
    let base = file.parent().unwrap_or(Path::new("."));
    let list = entries(&text, base);
    if list.is_empty() {
        return Err(Error::Invalid("no tracks found in the playlist".into()));
    }
    let mut rep = M3uImport {
        name: file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported playlist".into()),
        entries: list.len(),
        ..Default::default()
    };
    let mut to_import = Vec::new();
    for p in &list {
        if !p.is_file() {
            rep.missing.push(p.to_string_lossy().into_owned());
        } else if track_for(conn, p)?.is_some() {
            rep.in_library += 1;
        } else {
            to_import.push(p.clone());
        }
    }
    if !to_import.is_empty() {
        let r = library::import_paths(conn, &to_import, None, &mut |_| {})?;
        rep.imported = r.added + r.relinked;
        rep.my_tags = r.my_tags;
        rep.new_track_ids = r.added_track_ids;
    }
    let mut ids = Vec::new();
    for p in list.iter().filter(|p| p.is_file()) {
        if let Some(id) = track_for(conn, p)? {
            ids.push(id);
        }
    }
    if !ids.is_empty() {
        rep.playlist_id = Some(crate::playlist_match::create_playlist(
            conn, &rep.name, &ids,
        )?);
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_absolute_and_file_urls() {
        let base = Path::new("/music/lists");
        let e = entries(
            "\u{feff}#EXTM3U\n#EXTINF:300,A - B\nsub/a.mp3\n/abs/b.flac\nfile:///abs/c%20d.mp3\n\n",
            base,
        );
        assert_eq!(
            e,
            vec![
                PathBuf::from("/music/lists/sub/a.mp3"),
                PathBuf::from("/abs/b.flac"),
                PathBuf::from("/abs/c d.mp3"),
            ]
        );
    }
}
