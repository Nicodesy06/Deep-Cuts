//! Rekordbox XML ("rekordbox xml" bridge format) import and export.
//!
//! Only the documented XML exchange format is used; the encrypted
//! master.db is never read or written.

use crate::error::{Error, Result};
use crate::library::{self, audit};
use crate::util::{new_id, now_ms};
use quick_xml::events::{BytesStart, Event};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const SOURCE: &str = "import:rekordbox";

/// Where an imported library comes from: the field/cue/playlist source tag
/// (e.g. "import:traktor") and the key used in `external_ids`.
#[derive(Debug, Clone, Copy)]
pub struct Origin {
    pub source: &'static str,
    pub external: &'static str,
    pub audit_action: &'static str,
}

pub const REKORDBOX: Origin = Origin {
    source: SOURCE,
    external: "rekordbox",
    audit_action: "import-rekordbox",
};

// ---------------------------------------------------------------------------
// Location URIs
// ---------------------------------------------------------------------------

const PATH_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b':')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'(')
    .remove(b')')
    .remove(b'!')
    .remove(b'\'')
    .remove(b',');

pub fn path_to_location(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = if p.starts_with('/') {
        p
    } else {
        format!("/{p}")
    };
    format!(
        "file://localhost{}",
        percent_encoding::utf8_percent_encode(&p, PATH_SET)
    )
}

pub fn location_to_path(loc: &str) -> Option<String> {
    let rest = loc
        .strip_prefix("file://localhost")
        .or_else(|| loc.strip_prefix("file://"))?;
    let decoded = percent_encoding::percent_decode_str(rest)
        .decode_utf8()
        .ok()?
        .into_owned();
    // Windows: "/C:/Music/x.mp3" -> "C:/Music/x.mp3"
    let b = decoded.as_bytes();
    if b.len() > 3 && b[0] == b'/' && b[2] == b':' && b[1].is_ascii_alphabetic() {
        return Some(decoded[1..].replace('/', "\\"));
    }
    Some(decoded)
}

pub fn rating_to_stars(v: i64) -> i64 {
    match v {
        i64::MIN..=25 => 0,
        26..=76 => 1,
        77..=127 => 2,
        128..=178 => 3,
        179..=229 => 4,
        _ => 5,
    }
}

pub fn stars_to_rating(stars: i64) -> i64 {
    stars.clamp(0, 5) * 51
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct RbTrack {
    pub track_id: String,
    pub attrs: HashMap<String, String>,
    pub tempos: Vec<(f64, f64)>, // (inizio seconds, bpm)
    pub marks: Vec<RbMark>,
}

#[derive(Debug, Clone, Default)]
pub struct RbMark {
    pub name: String,
    pub kind: i64, // 0 cue, 4 loop
    pub start: f64,
    pub end: Option<f64>,
    pub num: i64, // -1 memory cue, >=0 hot cue slot
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RbPlaylist {
    /// Folder names from the root down to (excluding) this playlist.
    pub folders: Vec<String>,
    pub name: String,
    pub keys: Vec<String>,
    pub key_type: i64, // 0 = TrackID, 1 = Location
}

#[derive(Debug, Default)]
pub struct RbLibrary {
    pub tracks: Vec<RbTrack>,
    pub playlists: Vec<RbPlaylist>,
}

fn attrs_of(e: &BytesStart) -> Result<HashMap<String, String>> {
    let mut m = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Xml(e.to_string()))?;
        let key = a.key.as_ref().to_string();
        let val = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| Error::Xml(e.to_string()))?
            .into_owned();
        m.insert(key, val);
    }
    Ok(m)
}

pub fn parse(xml: &str) -> Result<RbLibrary> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut lib = RbLibrary::default();
    let mut in_collection = false;
    let mut current: Option<RbTrack> = None;
    // Stack of open NODE elements under PLAYLISTS: (name, type, playlist index)
    let mut nodes: Vec<(String, i64, Option<usize>)> = Vec::new();
    let mut saw_root = false;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| Error::Xml(format!("at byte {}: {e}", reader.buffer_position())))?;
        let (e, empty) = match &ev {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            Event::End(e) => {
                match e.name().as_ref() {
                    "COLLECTION" => in_collection = false,
                    "TRACK" => {
                        if let Some(t) = current.take() {
                            lib.tracks.push(t);
                        }
                    }
                    "NODE" => {
                        nodes.pop();
                    }
                    _ => {}
                }
                (None, false)
            }
            Event::Eof => break,
            _ => (None, false),
        };
        let Some(e) = e else { continue };
        match e.name().as_ref() {
            "DJ_PLAYLISTS" => saw_root = true,
            "COLLECTION" => in_collection = !empty,
            "TRACK" if in_collection => {
                let attrs = attrs_of(&e)?;
                let t = RbTrack {
                    track_id: attrs.get("TrackID").cloned().unwrap_or_default(),
                    attrs,
                    ..Default::default()
                };
                if empty {
                    lib.tracks.push(t);
                } else {
                    current = Some(t);
                }
            }
            "TEMPO" => {
                if let Some(t) = current.as_mut() {
                    let a = attrs_of(&e)?;
                    let inizio = a.get("Inizio").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    let bpm = a.get("Bpm").and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    if bpm > 0.0 {
                        t.tempos.push((inizio, bpm));
                    }
                }
            }
            "POSITION_MARK" => {
                if let Some(t) = current.as_mut() {
                    let a = attrs_of(&e)?;
                    let color = match (a.get("Red"), a.get("Green"), a.get("Blue")) {
                        (Some(r), Some(g), Some(b)) => Some(format!(
                            "#{:02x}{:02x}{:02x}",
                            r.parse::<u8>().unwrap_or(0),
                            g.parse::<u8>().unwrap_or(0),
                            b.parse::<u8>().unwrap_or(0)
                        )),
                        _ => None,
                    };
                    t.marks.push(RbMark {
                        name: a.get("Name").cloned().unwrap_or_default(),
                        kind: a.get("Type").and_then(|v| v.parse().ok()).unwrap_or(0),
                        start: a.get("Start").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                        end: a.get("End").and_then(|v| v.parse().ok()),
                        num: a.get("Num").and_then(|v| v.parse().ok()).unwrap_or(-1),
                        color,
                    });
                }
            }
            "NODE" => {
                let a = attrs_of(&e)?;
                let name = a.get("Name").cloned().unwrap_or_default();
                let ty: i64 = a.get("Type").and_then(|v| v.parse().ok()).unwrap_or(0);
                let mut idx = None;
                if ty == 1 {
                    let folders: Vec<String> = nodes.iter().skip(1).map(|n| n.0.clone()).collect();
                    lib.playlists.push(RbPlaylist {
                        folders,
                        name: name.clone(),
                        keys: vec![],
                        key_type: a.get("KeyType").and_then(|v| v.parse().ok()).unwrap_or(0),
                    });
                    idx = Some(lib.playlists.len() - 1);
                }
                if !empty {
                    nodes.push((name, ty, idx));
                }
            }
            "TRACK" => {
                // playlist entry
                if let Some((_, 1, Some(idx))) = nodes.last() {
                    let a = attrs_of(&e)?;
                    if let Some(k) = a.get("Key") {
                        lib.playlists[*idx].keys.push(k.clone());
                    }
                }
            }
            _ => {}
        }
    }
    if !saw_root {
        return Err(Error::Xml(
            "not a Rekordbox XML file (missing DJ_PLAYLISTS)".into(),
        ));
    }
    Ok(lib)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RbImportReport {
    pub tracks_in_xml: usize,
    pub matched_existing: usize,
    pub created: usize,
    pub missing_files: usize,
    pub cues_imported: usize,
    pub playlists_imported: usize,
    pub playlist_entries: usize,
    pub unresolved_playlist_entries: usize,
    pub warnings: Vec<String>,
    /// Rekordbox My Tags found in comments (`/* a / b */`), now tags.
    #[serde(default)]
    pub my_tags: Option<crate::mytag::MyTagReport>,
}

/// Import a Rekordbox XML. Read-only towards the XML and the audio files.
/// Values land with source 'import:rekordbox' (user edits keep priority).
pub fn import_xml(conn: &mut Connection, xml_path: &Path) -> Result<RbImportReport> {
    let xml = std::fs::read_to_string(xml_path)?;
    let lib = parse(&xml)?;
    let mut rep = import_library(conn, &lib, REKORDBOX, xml_path)?;
    rep.my_tags = Some(crate::mytag::import_from_comments(conn, SOURCE)?);
    Ok(rep)
}

/// Import a parsed DJ library (Rekordbox, Traktor, VirtualDJ, Serato all map
/// to `RbLibrary`). Track entries carry a `Location` file URI; playlists refer
/// to tracks by id (key_type 0) or by Location (key_type 1).
pub fn import_library(
    conn: &mut Connection,
    lib: &RbLibrary,
    origin: Origin,
    origin_file: &Path,
) -> Result<RbImportReport> {
    let mut rep = RbImportReport {
        tracks_in_xml: lib.tracks.len(),
        ..Default::default()
    };

    // First make sure every existing audio file is a library track (normal
    // import path => tags, hashes, relinking).
    let mut paths: Vec<(String, PathBuf)> = Vec::new();
    for t in &lib.tracks {
        match t.attrs.get("Location").and_then(|l| location_to_path(l)) {
            Some(p) => paths.push((t.track_id.clone(), PathBuf::from(p))),
            None => rep
                .warnings
                .push(format!("track {} has no usable Location", t.track_id)),
        }
    }
    let existing: Vec<PathBuf> = paths
        .iter()
        .filter_map(|p| library::existing_path(&p.1))
        .collect();
    library::import_paths(conn, &existing, None, &mut |_| {})?;

    let tx = crate::util::write_tx(conn)?;
    let mut rb_to_track: HashMap<String, String> = HashMap::new();
    let mut loc_to_track: HashMap<String, String> = HashMap::new();
    let now = now_ms();
    for t in &lib.tracks {
        let Some(loc) = t.attrs.get("Location") else {
            continue;
        };
        let Some(path) = location_to_path(loc) else {
            continue;
        };
        let key = library::path_key(Path::new(&path));
        let found: Option<String> = tx
            .query_row("SELECT track_id FROM files WHERE path = ?1", [&key], |r| {
                r.get(0)
            })
            .optional()?;
        let found = match found {
            Some(id) => Some(id),
            None => tx
                .query_row(
                    "SELECT track_id FROM external_ids WHERE source = ?2 AND external_id = ?1",
                    params![&t.track_id, origin.external],
                    |r| r.get(0),
                )
                .optional()?,
        };
        let track_id = match found {
            Some(id) => {
                rep.matched_existing += 1;
                id
            }
            None => {
                // File not on this machine: keep the entry as a missing track
                // so its metadata and cues are not lost and it can be relinked.
                let id = new_id();
                tx.execute(
                    "INSERT INTO tracks (id, created_at, updated_at) VALUES (?1, ?2, ?2)",
                    params![id, now],
                )?;
                tx.execute(
                    "INSERT INTO files (id, track_id, path, size, format, missing, last_seen, added_at) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
                    params![
                        new_id(),
                        id,
                        key,
                        t.attrs.get("Size").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
                        crate::util::extension(Path::new(&path)),
                        now
                    ],
                )?;
                rep.created += 1;
                rep.missing_files += 1;
                id
            }
        };
        tx.execute(
            "INSERT OR REPLACE INTO external_ids (track_id, source, external_id) VALUES (?1, ?3, ?2)",
            params![track_id, t.track_id, origin.external],
        )?;
        apply_track(&tx, &track_id, t, origin.source, &mut rep)?;
        library::refresh_fts(&tx, &track_id)?;
        rb_to_track.insert(t.track_id.clone(), track_id.clone());
        loc_to_track.insert(loc.clone(), track_id);
    }

    // Playlists: mirror the folder tree; re-import replaces contents of the
    // playlists that came from Rekordbox (never user-made ones).
    for pl in &lib.playlists {
        let mut parent: Option<String> = None;
        for folder in &pl.folders {
            parent = Some(find_or_create_playlist(
                &tx,
                folder,
                parent.as_deref(),
                "folder",
                origin.source,
            )?);
        }
        let pid =
            find_or_create_playlist(&tx, &pl.name, parent.as_deref(), "manual", origin.source)?;
        let mut ids = Vec::new();
        for k in &pl.keys {
            let resolved = if pl.key_type == 1 {
                loc_to_track.get(k)
            } else {
                rb_to_track.get(k)
            };
            match resolved {
                Some(id) => ids.push(id.clone()),
                None => rep.unresolved_playlist_entries += 1,
            }
        }
        rep.playlist_entries += ids.len();
        library::set_playlist_tracks(&tx, &pid, &ids)?;
        rep.playlists_imported += 1;
    }
    audit(
        &tx,
        origin.audit_action,
        "library",
        None,
        serde_json::json!({"file": origin_file, "tracks": rep.tracks_in_xml, "created": rep.created, "playlists": rep.playlists_imported}),
    )?;
    tx.commit()?;
    Ok(rep)
}

fn find_or_create_playlist(
    conn: &Connection,
    name: &str,
    parent: Option<&str>,
    kind: &str,
    source: &str,
) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM playlists WHERE name = ?1 AND parent_id IS ?2 AND source = ?3 AND kind = ?4",
            params![name, parent, source, kind],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO playlists (id, name, parent_id, kind, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![id, name, parent, kind, source, now_ms()],
    )?;
    Ok(id)
}

fn apply_track(
    conn: &Connection,
    track_id: &str,
    t: &RbTrack,
    source: &str,
    rep: &mut RbImportReport,
) -> Result<()> {
    let a = &t.attrs;
    let text = |k: &str| {
        a.get(k)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    for (attr, field) in [
        ("Name", "title"),
        ("Artist", "artist"),
        ("Album", "album"),
        ("Genre", "genre"),
        ("Label", "label"),
        ("Remixer", "remixer"),
        ("Comments", "comment"),
        ("Tonality", "key"),
        ("Mix", "version"),
    ] {
        match text(attr) {
            Some(v) => library::set_field(conn, track_id, field, source, Some(&v))?,
            None => library::clear_field_source(conn, track_id, field, source)?,
        }
    }
    if let Some(y) = text("Year")
        .and_then(|y| y.parse::<i64>().ok())
        .filter(|y| *y > 0)
    {
        library::set_field(conn, track_id, "year", source, Some(&y.to_string()))?;
    }
    if let Some(b) = text("AverageBpm")
        .and_then(|b| b.parse::<f64>().ok())
        .filter(|b| *b > 0.0)
    {
        library::set_field(conn, track_id, "bpm", source, Some(&b.to_string()))?;
    }
    if let Some(d) = text("TotalTime")
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| *d > 0.0)
    {
        library::set_field(
            conn,
            track_id,
            "durationMs",
            source,
            Some(&((d * 1000.0) as i64).to_string()),
        )?;
    }
    if let Some(r) = text("Rating").and_then(|r| r.parse::<i64>().ok()) {
        library::set_field(
            conn,
            track_id,
            "rating",
            source,
            Some(&rating_to_stars(r).to_string()),
        )?;
    }
    if let Some((inizio, bpm)) = t.tempos.first() {
        conn.execute(
            "INSERT INTO beatgrids (track_id, source, bpm, first_beat_ms, markers) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(track_id, source) DO UPDATE SET bpm = excluded.bpm, first_beat_ms = excluded.first_beat_ms, markers = excluded.markers",
            params![
                track_id,
                source,
                bpm,
                inizio * 1000.0,
                (t.tempos.len() > 1).then(|| serde_json::to_string(&t.tempos).unwrap_or_default())
            ],
        )?;
    }
    // Replace only cues that previously came from Rekordbox.
    conn.execute(
        "DELETE FROM cue_points WHERE track_id = ?1 AND source = ?2 AND locked = 0",
        params![track_id, source],
    )?;
    for m in &t.marks {
        let kind = match (m.kind, m.num) {
            (4, _) => "loop",
            (_, n) if n >= 0 => "hotcue",
            _ => "memory",
        };
        conn.execute(
            "INSERT INTO cue_points (id, track_id, kind, slot, start_ms, end_ms, name, color, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                new_id(),
                track_id,
                kind,
                (m.num >= 0).then_some(m.num),
                m.start * 1000.0,
                m.end.map(|e| e * 1000.0),
                (!m.name.is_empty()).then_some(m.name.clone()),
                m.color,
                source,
                now_ms()
            ],
        )?;
        rep.cues_imported += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

fn kind_name(format: Option<&str>) -> &'static str {
    match format.unwrap_or("") {
        "mp3" => "MP3 File",
        "wav" => "WAV File",
        "aif" | "aiff" => "AIFF File",
        "flac" => "FLAC File",
        "m4a" | "mp4" | "aac" | "alac" => "M4A File",
        _ => "Audio File",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    pub id: String,
    pub kind: String,
    pub slot: Option<i64>,
    pub start_ms: f64,
    pub end_ms: Option<f64>,
    pub name: Option<String>,
    pub color: Option<String>,
    pub source: String,
    pub locked: bool,
}

pub fn cues(conn: &Connection, track_id: &str) -> Result<Vec<Cue>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, slot, start_ms, end_ms, name, color, source, locked FROM cue_points WHERE track_id = ?1 ORDER BY start_ms",
    )?;
    let rows = stmt.query_map([track_id], |r| {
        Ok(Cue {
            id: r.get(0)?,
            kind: r.get(1)?,
            slot: r.get(2)?,
            start_ms: r.get(3)?,
            end_ms: r.get(4)?,
            name: r.get(5)?,
            color: r.get(6)?,
            source: r.get(7)?,
            locked: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn build_xml(conn: &Connection, ids: &[String], playlist_name: &str) -> Result<String> {
    Ok(build_xml_lib(&crate::djlib::for_ids(
        conn,
        ids,
        playlist_name,
    )?))
}

/// Folder tree of playlists for writers that nest NODEs.
pub(crate) enum Node<'a> {
    Folder(String, Vec<Node<'a>>),
    Playlist(&'a crate::djlib::XPlaylist),
}

pub(crate) fn playlist_tree(lib: &crate::djlib::ExportLibrary) -> Vec<Node<'_>> {
    fn insert<'a>(nodes: &mut Vec<Node<'a>>, folders: &[String], p: &'a crate::djlib::XPlaylist) {
        match folders.split_first() {
            None => nodes.push(Node::Playlist(p)),
            Some((first, rest)) => {
                let pos = nodes
                    .iter()
                    .position(|n| matches!(n, Node::Folder(name, _) if name == first));
                let i = match pos {
                    Some(i) => i,
                    None => {
                        nodes.push(Node::Folder(first.clone(), Vec::new()));
                        nodes.len() - 1
                    }
                };
                if let Node::Folder(_, children) = &mut nodes[i] {
                    insert(children, rest, p);
                }
            }
        }
    }
    let mut root = Vec::new();
    for p in &lib.playlists {
        insert(&mut root, &p.folders, p);
    }
    root
}

pub fn build_xml_lib(lib: &crate::djlib::ExportLibrary) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n",
    );
    s.push_str(&format!(
        "  <PRODUCT Name=\"Cradex\" Version=\"{}\" Company=\"Cradex\"/>\n",
        env!("CARGO_PKG_VERSION")
    ));
    s.push_str(&format!(
        "  <COLLECTION Entries=\"{}\">\n",
        lib.tracks.len()
    ));
    for (i, x) in lib.tracks.iter().enumerate() {
        let t = &x.row;
        let added = chrono::DateTime::from_timestamp_millis(t.created_at)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let attr = |k: &str, v: &str| format!(" {k}=\"{}\"", esc(v));
        let mut line = format!("    <TRACK TrackID=\"{}\"", i + 1);
        line += &attr("Name", t.title.as_deref().unwrap_or(""));
        line += &attr("Artist", t.artist.as_deref().unwrap_or(""));
        line += &attr("Album", t.album.as_deref().unwrap_or(""));
        line += &attr("Genre", t.genre.as_deref().unwrap_or(""));
        line += &attr("Kind", kind_name(t.format.as_deref()));
        line += &attr("Size", &x.size.to_string());
        line += &attr(
            "TotalTime",
            &t.duration_ms
                .map(|d| (d / 1000).to_string())
                .unwrap_or_else(|| "0".into()),
        );
        line += &attr("Year", &t.year.map(|y| y.to_string()).unwrap_or_default());
        line += &attr(
            "AverageBpm",
            &t.bpm
                .map(|b| format!("{b:.2}"))
                .unwrap_or_else(|| "0.00".into()),
        );
        line += &attr("DateAdded", &added);
        line += &attr("BitRate", &t.bitrate.unwrap_or(0).to_string());
        line += &attr("SampleRate", &x.sample_rate.unwrap_or(0).to_string());
        line += &attr("Comments", t.comment.as_deref().unwrap_or(""));
        line += &attr(
            "Rating",
            &stars_to_rating(t.rating.unwrap_or(0)).to_string(),
        );
        line += &attr(
            "Location",
            &path_to_location(t.path.as_deref().unwrap_or("")),
        );
        line += &attr("Remixer", t.remixer.as_deref().unwrap_or(""));
        line += &attr("Tonality", t.key.as_deref().unwrap_or(""));
        line += &attr("Label", t.label.as_deref().unwrap_or(""));
        line += &attr("Mix", t.version.as_deref().unwrap_or(""));
        line.push('>');
        s.push_str(&line);
        s.push('\n');
        if let Some((bpm, first)) = x.grid {
            s.push_str(&format!(
                "      <TEMPO Inizio=\"{:.3}\" Bpm=\"{bpm:.2}\" Metro=\"4/4\" Battito=\"1\"/>\n",
                first / 1000.0
            ));
        }
        for c in &x.cues {
            let (ty, end) = match (c.kind.as_str(), c.end_ms) {
                ("loop", Some(e)) => (4, format!(" End=\"{:.3}\"", e / 1000.0)),
                _ => (0, String::new()),
            };
            let num = if c.kind == "hotcue" {
                c.slot.unwrap_or(0)
            } else {
                -1
            };
            let rgb = c
                .color
                .as_deref()
                .and_then(|h| {
                    let h = h.trim_start_matches('#');
                    (h.len() == 6).then(|| {
                        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
                        format!(" Red=\"{}\" Green=\"{}\" Blue=\"{}\"", p(0), p(2), p(4))
                    })
                })
                .unwrap_or_default();
            s.push_str(&format!(
                "      <POSITION_MARK Name=\"{}\" Type=\"{ty}\" Start=\"{:.3}\"{end} Num=\"{num}\"{rgb}/>\n",
                esc(c.name.as_deref().unwrap_or("")),
                c.start_ms / 1000.0
            ));
        }
        s.push_str("    </TRACK>\n");
    }
    fn write_nodes(s: &mut String, nodes: &[Node], depth: usize) {
        let pad = "  ".repeat(depth);
        for n in nodes {
            match n {
                Node::Folder(name, children) => {
                    s.push_str(&format!(
                        "{pad}<NODE Type=\"0\" Name=\"{}\" Count=\"{}\">\n",
                        esc(name),
                        children.len()
                    ));
                    write_nodes(s, children, depth + 1);
                    s.push_str(&format!("{pad}</NODE>\n"));
                }
                Node::Playlist(p) => {
                    s.push_str(&format!(
                        "{pad}<NODE Name=\"{}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">\n",
                        esc(&p.name),
                        p.tracks.len()
                    ));
                    for i in &p.tracks {
                        s.push_str(&format!("{pad}  <TRACK Key=\"{}\"/>\n", i + 1));
                    }
                    s.push_str(&format!("{pad}</NODE>\n"));
                }
            }
        }
    }
    let tree = playlist_tree(lib);
    s.push_str(&format!(
        "  </COLLECTION>\n  <PLAYLISTS>\n    <NODE Type=\"0\" Name=\"ROOT\" Count=\"{}\">\n",
        tree.len()
    ));
    write_nodes(&mut s, &tree, 3);
    s.push_str("    </NODE>\n  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        let l = path_to_location("/Users/dj/Music/Ça va (Extended Mix).mp3");
        assert_eq!(
            l,
            "file://localhost/Users/dj/Music/%C3%87a%20va%20(Extended%20Mix).mp3"
        );
        assert_eq!(
            location_to_path(&l).unwrap(),
            "/Users/dj/Music/Ça va (Extended Mix).mp3"
        );
        assert_eq!(
            location_to_path("file://localhost/C:/Music/a%20b.mp3").unwrap(),
            "C:\\Music\\a b.mp3"
        );
        assert_eq!(
            path_to_location("C:\\Music\\a b.mp3"),
            "file://localhost/C:/Music/a%20b.mp3"
        );
    }

    #[test]
    fn ratings() {
        for s in 0..=5 {
            assert_eq!(rating_to_stars(stars_to_rating(s)), s);
        }
    }

    #[test]
    fn parse_minimal() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0"><PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
<COLLECTION Entries="1">
<TRACK TrackID="7" Name="A &amp; B" Artist="X" AverageBpm="124.00" Tonality="Am" Rating="204" Location="file://localhost/tmp/x.mp3">
<TEMPO Inizio="0.120" Bpm="124.00" Metro="4/4" Battito="1"/>
<POSITION_MARK Name="Drop" Type="0" Start="60.5" Num="1" Red="255" Green="0" Blue="0"/>
<POSITION_MARK Name="" Type="4" Start="10" End="20" Num="-1"/>
</TRACK></COLLECTION>
<PLAYLISTS><NODE Type="0" Name="ROOT" Count="1"><NODE Type="0" Name="Gigs" Count="1">
<NODE Name="Friday" Type="1" KeyType="0" Entries="1"><TRACK Key="7"/></NODE></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#;
        let lib = parse(xml).unwrap();
        assert_eq!(lib.tracks.len(), 1);
        let t = &lib.tracks[0];
        assert_eq!(t.attrs["Name"], "A & B");
        assert_eq!(t.tempos, vec![(0.12, 124.0)]);
        assert_eq!(t.marks.len(), 2);
        assert_eq!(t.marks[0].color.as_deref(), Some("#ff0000"));
        assert_eq!(t.marks[1].end, Some(20.0));
        assert_eq!(lib.playlists.len(), 1);
        assert_eq!(lib.playlists[0].folders, vec!["Gigs".to_string()]);
        assert_eq!(lib.playlists[0].keys, vec!["7".to_string()]);
        assert!(parse("<foo/>").is_err());
    }
}
