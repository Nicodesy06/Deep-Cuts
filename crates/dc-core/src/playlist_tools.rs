//! Playlist tools: combine (union / intersection / difference), duplicate,
//! shuffle, name prefixes and "which playlists contain this track".
//! Combining always creates a new playlist; the sources are not changed.

use crate::error::{Error, Result};
use crate::library;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Combine {
    /// All tracks of all playlists, in order, without repeats.
    Union,
    /// Tracks present in every playlist (order of the first).
    Intersect,
    /// Tracks of the first playlist that are in none of the others.
    Subtract,
}

pub fn combined_ids(conn: &Connection, ids: &[String], op: Combine) -> Result<Vec<String>> {
    let lists: Vec<Vec<String>> = ids
        .iter()
        .map(|id| library::playlist_track_ids(conn, id))
        .collect::<Result<_>>()?;
    let Some(first) = lists.first() else {
        return Err(Error::Invalid("no playlists given".into()));
    };
    let mut out: Vec<String> = Vec::new();
    match op {
        Combine::Union => {
            for t in lists.iter().flatten() {
                if !out.contains(t) {
                    out.push(t.clone());
                }
            }
        }
        Combine::Intersect => {
            for t in first {
                if lists[1..].iter().all(|l| l.contains(t)) && !out.contains(t) {
                    out.push(t.clone());
                }
            }
        }
        Combine::Subtract => {
            for t in first {
                if !lists[1..].iter().any(|l| l.contains(t)) && !out.contains(t) {
                    out.push(t.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Create a new manual playlist from a combination of playlists.
pub fn combine(conn: &Connection, name: &str, ids: &[String], op: Combine) -> Result<String> {
    let tracks = combined_ids(conn, ids, op)?;
    let id = library::create_playlist(conn, name, None, "manual", None)?;
    library::set_playlist_tracks(conn, &id, &tracks)?;
    Ok(id)
}

/// Shuffle a playlist (undoable). `seed` makes the order reproducible.
pub fn shuffle(conn: &Connection, id: &str, seed: u64) -> Result<()> {
    let mut ids = library::playlist_track_ids(conn, id)?;
    let mut state = seed | 1;
    for i in (1..ids.len()).rev() {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545F4914F6CDD1D);
        ids.swap(i, (r % (i as u64 + 1)) as usize);
    }
    library::reorder_playlist(conn, id, &ids, "playlist-shuffle")
}

/// Prepend `prefix` to the names of playlists (skips names that already
/// start with it).
pub fn add_prefix(conn: &Connection, ids: &[String], prefix: &str) -> Result<usize> {
    let mut n = 0;
    for id in ids {
        let name: String =
            conn.query_row("SELECT name FROM playlists WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        if !name.starts_with(prefix) {
            library::rename_playlist(conn, id, &format!("{prefix}{name}"))?;
            n += 1;
        }
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// 1-based positions of the track in the playlist.
    pub positions: Vec<i64>,
}

/// Manual playlists and sets that contain a track.
pub fn occurrences(conn: &Connection, track_id: &str) -> Result<Vec<Occurrence>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.kind, group_concat(pt.position + 1) FROM playlist_tracks pt
         JOIN playlists p ON p.id = pt.playlist_id WHERE pt.track_id = ?1
         GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([track_id], |r| {
        let pos: String = r.get(3)?;
        Ok(Occurrence {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            positions: pos.split(',').filter_map(|p| p.parse().ok()).collect(),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
