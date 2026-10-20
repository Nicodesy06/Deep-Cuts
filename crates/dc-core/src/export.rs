//! Playlist / library export: M3U8, CSV and Rekordbox XML.

use crate::error::Result;
use crate::library::{self, TrackRow};
use crate::rekordbox;
use rusqlite::Connection;
use std::io::Write;
use std::path::Path;

fn tracks_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<TrackRow>> {
    ids.iter().map(|id| library::get_track(conn, id)).collect()
}

pub fn m3u8_string(tracks: &[TrackRow]) -> String {
    let mut s = String::from("#EXTM3U\n");
    for t in tracks {
        let Some(path) = &t.path else { continue };
        let secs = t
            .duration_ms
            .map(|d| (d as f64 / 1000.0).round() as i64)
            .unwrap_or(-1);
        let artist = t.artist.as_deref().unwrap_or("");
        let title = t.title.as_deref().unwrap_or("");
        let label = if artist.is_empty() {
            title.to_string()
        } else {
            format!("{artist} - {title}")
        };
        s.push_str(&format!(
            "#EXTINF:{secs},{}\n{}\n",
            label.replace(['\n', '\r'], " "),
            path
        ));
    }
    s
}

pub fn export_m3u8(conn: &Connection, ids: &[String], out: &Path) -> Result<usize> {
    let tracks = tracks_by_ids(conn, ids)?;
    write_atomic(out, m3u8_string(&tracks).as_bytes())?;
    Ok(tracks.len())
}

pub fn export_csv(conn: &Connection, ids: &[String], out: &Path) -> Result<usize> {
    let tracks = tracks_by_ids(conn, ids)?;
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record([
        "#", "Artist", "Title", "Version", "Remixer", "Album", "Label", "Genre", "Year", "BPM",
        "Key", "Camelot", "Duration", "Rating", "Decision", "LUFS", "Comment", "Path",
    ])?;
    for (i, t) in tracks.iter().enumerate() {
        let dur = t
            .duration_ms
            .map(|d| format!("{}:{:02}", d / 60000, (d / 1000) % 60))
            .unwrap_or_default();
        w.write_record([
            (i + 1).to_string(),
            t.artist.clone().unwrap_or_default(),
            t.title.clone().unwrap_or_default(),
            t.version.clone().unwrap_or_default(),
            t.remixer.clone().unwrap_or_default(),
            t.album.clone().unwrap_or_default(),
            t.label.clone().unwrap_or_default(),
            t.genre.clone().unwrap_or_default(),
            t.year.map(|v| v.to_string()).unwrap_or_default(),
            t.bpm.map(|v| format!("{v:.2}")).unwrap_or_default(),
            t.key.clone().unwrap_or_default(),
            t.camelot.clone().unwrap_or_default(),
            dur,
            t.rating.map(|v| v.to_string()).unwrap_or_default(),
            t.decision.clone().unwrap_or_default(),
            t.lufs.map(|v| format!("{v:.1}")).unwrap_or_default(),
            t.comment.clone().unwrap_or_default(),
            t.path.clone().unwrap_or_default(),
        ])?;
    }
    let bytes = w
        .into_inner()
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    write_atomic(out, &bytes)?;
    Ok(tracks.len())
}

/// Export tracks (and one playlist containing them, in order) as a
/// Rekordbox-compatible XML collection, importable via Rekordbox
/// Preferences > Advanced > rekordbox xml.
pub fn export_rekordbox_xml(
    conn: &Connection,
    ids: &[String],
    playlist_name: &str,
    out: &Path,
) -> Result<usize> {
    let xml = rekordbox::build_xml(conn, ids, playlist_name)?;
    write_atomic(out, xml.as_bytes())?;
    Ok(ids.len())
}

/// Write via a temporary file + rename so a crash never leaves half a file.
pub fn write_atomic(out: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = out.with_extension("dc-tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(())
}
