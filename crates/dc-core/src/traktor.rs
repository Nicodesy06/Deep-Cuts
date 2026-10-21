//! Traktor collection (NML) import and export.
//!
//! The NML written here is a separate collection file that Traktor opens via
//! File > Import Collection / Import Another Collection; Traktor's own
//! collection.nml is never modified.

use crate::djlib::{ExportLibrary, XTrack, esc};
use crate::error::{Error, Result};
use crate::keys::Key;
use crate::rekordbox::{
    Node, RbLibrary, RbMark, RbPlaylist, RbTrack, path_to_location, playlist_tree,
};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

pub const ORIGIN: crate::rekordbox::Origin = crate::rekordbox::Origin {
    source: "import:traktor",
    external: "traktor",
    audit_action: "import-traktor",
};

/// Traktor MUSICAL_KEY value: 0–11 = C…B major, 12–23 = Cm…Bm.
pub fn key_value(k: &Key) -> u8 {
    k.tonic % 12 + if k.minor { 12 } else { 0 }
}

pub fn key_from_value(v: i64) -> Option<Key> {
    (0..24).contains(&v).then_some(Key {
        tonic: (v % 12) as u8,
        minor: v >= 12,
    })
}

/// (VOLUME, DIR, FILE) for a path. macOS paths under /Volumes/<name> use
/// that volume; other absolute paths use `root_volume` (the startup disk
/// name on macOS). Windows paths use the drive ("C:").
pub fn location_parts(path: &str, root_volume: &str) -> (String, String, String) {
    let p = path.replace('\\', "/");
    let (volume, rest) = if p.len() > 2 && p.as_bytes()[1] == b':' {
        (p[..2].to_string(), p[2..].to_string())
    } else if let Some(r) = p.strip_prefix("/Volumes/") {
        match r.split_once('/') {
            Some((v, rest)) => (v.to_string(), format!("/{rest}")),
            None => (r.to_string(), "/".into()),
        }
    } else {
        (root_volume.to_string(), p.clone())
    };
    let (dir, file) = match rest.rsplit_once('/') {
        Some((d, f)) => (d.to_string(), f.to_string()),
        None => (String::new(), rest),
    };
    let dir = dir
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| format!("/:{s}"))
        .collect::<String>()
        + "/:";
    (volume, dir, file)
}

/// Absolute path from NML location parts (inverse of `location_parts`).
pub fn path_from_location(volume: &str, dir: &str, file: &str, root_volume: &str) -> String {
    let dir: String = dir
        .split("/:")
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    let rel = if dir.is_empty() {
        file.to_string()
    } else {
        format!("{dir}/{file}")
    };
    if volume.len() == 2 && volume.ends_with(':') {
        format!("{volume}\\{}", rel.replace('/', "\\"))
    } else if volume.is_empty() || volume == root_volume {
        format!("/{rel}")
    } else {
        format!("/Volumes/{volume}/{rel}")
    }
}

/// The macOS startup disk name (the /Volumes entry that points at "/"),
/// "Macintosh HD" if it cannot be determined.
pub fn root_volume_name() -> String {
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            if std::fs::canonicalize(e.path()).ok().as_deref() == Some(std::path::Path::new("/")) {
                return e.file_name().to_string_lossy().into_owned();
            }
        }
    }
    "Macintosh HD".into()
}

fn primary_key(path: &str, root_volume: &str) -> String {
    let (v, d, f) = location_parts(path, root_volume);
    format!("{v}{d}{f}")
}

fn entry(s: &mut String, x: &XTrack, root_volume: &str) {
    let t = &x.row;
    let path = t.path.as_deref().unwrap_or("");
    let (volume, dir, file) = location_parts(path, root_volume);
    let a = |v: Option<&str>| esc(v.unwrap_or(""));
    s.push_str(&format!(
        "<ENTRY MODIFIED_DATE=\"{}\" MODIFIED_TIME=\"0\" TITLE=\"{}\" ARTIST=\"{}\">\n",
        chrono::Local::now().format("%Y/%-m/%-d"),
        a(t.title.as_deref()),
        a(t.artist.as_deref())
    ));
    s.push_str(&format!(
        "<LOCATION DIR=\"{}\" FILE=\"{}\" VOLUME=\"{}\" VOLUMEID=\"{}\"></LOCATION>\n",
        esc(&dir),
        esc(&file),
        esc(&volume),
        esc(&volume)
    ));
    if t.album.is_some() {
        s.push_str(&format!(
            "<ALBUM TITLE=\"{}\"></ALBUM>\n",
            a(t.album.as_deref())
        ));
    }
    s.push_str("<MODIFICATION_INFO AUTHOR_TYPE=\"user\"></MODIFICATION_INFO>\n");
    let mut info = format!(
        "<INFO BITRATE=\"{}\" GENRE=\"{}\" LABEL=\"{}\" COMMENT=\"{}\" REMIXER=\"{}\" MIX=\"{}\" KEY=\"{}\" RANKING=\"{}\" FILESIZE=\"{}\"",
        t.bitrate.unwrap_or(0) * 1000,
        a(t.genre.as_deref()),
        a(t.label.as_deref()),
        a(t.comment.as_deref()),
        a(t.remixer.as_deref()),
        a(t.version.as_deref()),
        a(t.camelot.as_deref()),
        t.rating.unwrap_or(0).clamp(0, 5) * 51,
        x.size / 1024
    );
    if let Some(d) = t.duration_ms {
        info += &format!(
            " PLAYTIME=\"{}\" PLAYTIME_FLOAT=\"{:.6}\"",
            d / 1000,
            d as f64 / 1000.0
        );
    }
    if let Some(y) = t.year {
        info += &format!(" RELEASE_DATE=\"{y}/1/1\"");
    }
    info += "></INFO>\n";
    s.push_str(&info);
    let bpm = x.grid.map(|g| g.0).or(t.bpm);
    if let Some(b) = bpm {
        s.push_str(&format!(
            "<TEMPO BPM=\"{b:.6}\" BPM_QUALITY=\"100.000000\"></TEMPO>\n"
        ));
    }
    if let Some(k) = t.key.as_deref().and_then(Key::parse) {
        s.push_str(&format!(
            "<MUSICAL_KEY VALUE=\"{}\"></MUSICAL_KEY>\n",
            key_value(&k)
        ));
    }
    if let Some((_, first)) = x.grid {
        s.push_str(&format!(
            "<CUE_V2 NAME=\"AutoGrid\" DISPL_ORDER=\"0\" TYPE=\"4\" START=\"{first:.6}\" LEN=\"0.000000\" REPEATS=\"-1\" HOTCUE=\"-1\"></CUE_V2>\n"
        ));
    }
    for c in &x.cues {
        let (ty, len) = match (c.kind.as_str(), c.end_ms) {
            ("loop", Some(e)) => (5, e - c.start_ms),
            _ => (0, 0.0),
        };
        let hot = if c.kind == "hotcue" {
            c.slot.unwrap_or(0)
        } else {
            -1
        };
        s.push_str(&format!(
            "<CUE_V2 NAME=\"{}\" DISPL_ORDER=\"0\" TYPE=\"{ty}\" START=\"{:.6}\" LEN=\"{len:.6}\" REPEATS=\"-1\" HOTCUE=\"{hot}\"></CUE_V2>\n",
            esc(c.name.as_deref().unwrap_or("n.n.")),
            c.start_ms
        ));
    }
    s.push_str("</ENTRY>\n");
}

pub fn build_nml(lib: &ExportLibrary, root_volume: &str) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<NML VERSION=\"19\"><HEAD COMPANY=\"www.native-instruments.com\" PROGRAM=\"Traktor\"></HEAD>\n<MUSICFOLDERS></MUSICFOLDERS>\n",
    );
    s.push_str(&format!("<COLLECTION ENTRIES=\"{}\">\n", lib.tracks.len()));
    for x in &lib.tracks {
        entry(&mut s, x, root_volume);
    }
    s.push_str("</COLLECTION>\n<SETS ENTRIES=\"0\"></SETS>\n<PLAYLISTS>\n");
    fn nodes(s: &mut String, lib: &ExportLibrary, list: &[Node], root_volume: &str) {
        for n in list {
            match n {
                Node::Folder(name, children) => {
                    s.push_str(&format!(
                        "<NODE TYPE=\"FOLDER\" NAME=\"{}\"><SUBNODES COUNT=\"{}\">\n",
                        esc(name),
                        children.len()
                    ));
                    nodes(s, lib, children, root_volume);
                    s.push_str("</SUBNODES></NODE>\n");
                }
                Node::Playlist(p) => {
                    s.push_str(&format!(
                        "<NODE TYPE=\"PLAYLIST\" NAME=\"{}\"><PLAYLIST ENTRIES=\"{}\" TYPE=\"LIST\" UUID=\"{}\">\n",
                        esc(&p.name),
                        p.tracks.len(),
                        uuid::Uuid::new_v4().simple()
                    ));
                    for i in &p.tracks {
                        let path = lib.tracks[*i].row.path.as_deref().unwrap_or("");
                        s.push_str(&format!(
                            "<ENTRY><PRIMARYKEY TYPE=\"TRACK\" KEY=\"{}\"></PRIMARYKEY></ENTRY>\n",
                            esc(&primary_key(path, root_volume))
                        ));
                    }
                    s.push_str("</PLAYLIST></NODE>\n");
                }
            }
        }
    }
    let tree = playlist_tree(lib);
    s.push_str(&format!(
        "<NODE TYPE=\"FOLDER\" NAME=\"$ROOT\"><SUBNODES COUNT=\"{}\">\n",
        tree.len()
    ));
    nodes(&mut s, lib, &tree, root_volume);
    s.push_str("</SUBNODES></NODE>\n</PLAYLISTS>\n</NML>\n");
    s
}

fn attrs(e: &BytesStart) -> Result<HashMap<String, String>> {
    let mut m = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Xml(e.to_string()))?;
        m.insert(
            a.key.as_ref().to_string(),
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| Error::Xml(e.to_string()))?
                .into_owned(),
        );
    }
    Ok(m)
}

/// Parse an NML collection into the neutral import structure.
pub fn parse_nml(xml: &str, root_volume: &str) -> Result<RbLibrary> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut lib = RbLibrary::default();
    let mut saw_nml = false;
    let mut in_collection = false;
    let mut cur: Option<(RbTrack, Option<f64>)> = None;
    let mut folder_stack: Vec<String> = Vec::new();
    let mut node_kinds: Vec<bool> = Vec::new(); // true = folder
    let mut cur_pl: Option<RbPlaylist> = None;
    let mut n = 0usize;
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
                    "ENTRY" if in_collection => {
                        if let Some((mut t, bpm)) = cur.take() {
                            if let (Some(b), true) = (bpm, t.tempos.is_empty()) {
                                t.attrs.insert("AverageBpm".into(), b.to_string());
                            }
                            lib.tracks.push(t);
                        }
                    }
                    "NODE" => {
                        if node_kinds.pop() == Some(true) {
                            folder_stack.pop();
                        } else if let Some(p) = cur_pl.take() {
                            lib.playlists.push(p);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let Some(e) = e else { continue };
        let a = attrs(&e)?;
        match e.name().as_ref() {
            "NML" => saw_nml = true,
            "COLLECTION" => in_collection = true,
            "ENTRY" if in_collection => {
                n += 1;
                let mut t = RbTrack {
                    track_id: format!("nml-{n}"),
                    ..Default::default()
                };
                for (from, to) in [("TITLE", "Name"), ("ARTIST", "Artist")] {
                    if let Some(v) = a.get(from) {
                        t.attrs.insert(to.into(), v.clone());
                    }
                }
                cur = Some((t, None));
            }
            "LOCATION" => {
                if let Some((t, _)) = cur.as_mut() {
                    let path = path_from_location(
                        a.get("VOLUME").map(String::as_str).unwrap_or(""),
                        a.get("DIR").map(String::as_str).unwrap_or(""),
                        a.get("FILE").map(String::as_str).unwrap_or(""),
                        root_volume,
                    );
                    t.track_id = primary_key(&path, root_volume);
                    t.attrs.insert("Location".into(), path_to_location(&path));
                }
            }
            "ALBUM" => {
                if let (Some((t, _)), Some(v)) = (cur.as_mut(), a.get("TITLE")) {
                    t.attrs.insert("Album".into(), v.clone());
                }
            }
            "INFO" => {
                if let Some((t, _)) = cur.as_mut() {
                    for (from, to) in [
                        ("GENRE", "Genre"),
                        ("LABEL", "Label"),
                        ("COMMENT", "Comments"),
                        ("REMIXER", "Remixer"),
                        ("MIX", "Mix"),
                        ("RANKING", "Rating"),
                        ("PLAYTIME", "TotalTime"),
                    ] {
                        if let Some(v) = a.get(from) {
                            t.attrs.insert(to.into(), v.clone());
                        }
                    }
                    if let Some(y) = a.get("RELEASE_DATE").and_then(|d| d.split('/').next()) {
                        t.attrs.insert("Year".into(), y.to_string());
                    }
                    if !t.attrs.contains_key("Tonality")
                        && let Some(k) = a.get("KEY").and_then(|k| Key::parse(k))
                    {
                        t.attrs.insert("Tonality".into(), k.name());
                    }
                }
            }
            "TEMPO" => {
                if let Some((_, bpm)) = cur.as_mut() {
                    *bpm = a
                        .get("BPM")
                        .and_then(|b| b.parse().ok())
                        .filter(|b: &f64| *b > 0.0);
                }
            }
            "MUSICAL_KEY" => {
                if let (Some((t, _)), Some(k)) = (
                    cur.as_mut(),
                    a.get("VALUE")
                        .and_then(|v| v.parse().ok())
                        .and_then(key_from_value),
                ) {
                    t.attrs.insert("Tonality".into(), k.name());
                }
            }
            "CUE_V2" => {
                if let Some((t, bpm)) = cur.as_mut() {
                    let start = a
                        .get("START")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        / 1000.0;
                    let len = a
                        .get("LEN")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0)
                        / 1000.0;
                    let hot = a
                        .get("HOTCUE")
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(-1);
                    let name = a.get("NAME").cloned().unwrap_or_default();
                    match a.get("TYPE").map(String::as_str) {
                        Some("4") => {
                            if let Some(b) = *bpm {
                                t.tempos.push((start, b));
                                t.attrs.insert("AverageBpm".into(), b.to_string());
                            }
                        }
                        Some("5") if len > 0.0 => t.marks.push(RbMark {
                            name,
                            kind: 4,
                            start,
                            end: Some(start + len),
                            num: hot,
                            color: None,
                        }),
                        Some("0") => t.marks.push(RbMark {
                            name: if name == "n.n." { String::new() } else { name },
                            kind: 0,
                            start,
                            end: None,
                            num: hot,
                            color: None,
                        }),
                        _ => {}
                    }
                }
            }
            "NODE" if !in_collection => {
                let ty = a.get("TYPE").map(String::as_str).unwrap_or("");
                let name = a.get("NAME").cloned().unwrap_or_default();
                if ty == "FOLDER" {
                    if !empty {
                        node_kinds.push(true);
                        folder_stack.push(name);
                    }
                } else if !empty {
                    node_kinds.push(false);
                    cur_pl = Some(RbPlaylist {
                        folders: folder_stack.iter().skip(1).cloned().collect(), // skip $ROOT
                        name,
                        keys: Vec::new(),
                        key_type: 0,
                    });
                }
            }
            "PRIMARYKEY" => {
                if let (Some(p), Some(k)) = (cur_pl.as_mut(), a.get("KEY")) {
                    p.keys.push(k.clone());
                }
            }
            _ => {}
        }
        if empty
            && e.name().as_ref() == "ENTRY"
            && in_collection
            && let Some((t, _)) = cur.take()
        {
            lib.tracks.push(t);
        }
    }
    if !saw_nml {
        return Err(Error::Xml("not a Traktor NML file".into()));
    }
    // Playlist keys are "VOLUME/:DIR/:FILE" primary keys = track ids above.
    lib.playlists.retain(|p| !p.name.starts_with('_'));
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_round_trip() {
        let (v, d, f) = location_parts("/Users/dj/Music/House/a b.mp3", "Macintosh HD");
        assert_eq!(
            (v.as_str(), d.as_str(), f.as_str()),
            ("Macintosh HD", "/:Users/:dj/:Music/:House/:", "a b.mp3")
        );
        assert_eq!(
            path_from_location(&v, &d, &f, "Macintosh HD"),
            "/Users/dj/Music/House/a b.mp3"
        );
        let (v, d, f) = location_parts("/Volumes/USB/Tracks/x.flac", "Macintosh HD");
        assert_eq!(
            (v.as_str(), d.as_str(), f.as_str()),
            ("USB", "/:Tracks/:", "x.flac")
        );
        assert_eq!(
            path_from_location(&v, &d, &f, "Macintosh HD"),
            "/Volumes/USB/Tracks/x.flac"
        );
        let (v, d, f) = location_parts("C:\\Music\\x.mp3", "");
        assert_eq!(
            (v.as_str(), d.as_str(), f.as_str()),
            ("C:", "/:Music/:", "x.mp3")
        );
        assert_eq!(path_from_location(&v, &d, &f, ""), "C:\\Music\\x.mp3");
    }

    #[test]
    fn keys() {
        for v in 0..24 {
            assert_eq!(key_value(&key_from_value(v).unwrap()) as i64, v);
        }
        assert_eq!(key_value(&Key::parse("Am").unwrap()), 21);
        assert_eq!(key_value(&Key::parse("C").unwrap()), 0);
    }
}
