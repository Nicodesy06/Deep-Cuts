//! VirtualDJ: read `database.xml` (metadata, cues, beatgrid) and playlists
//! (`.vdjfolder`), write playlists as `.vdjfolder` files. VirtualDJ's own
//! database is never modified.

use crate::djlib::{ExportLibrary, esc};
use crate::error::{Error, Result};
use crate::keys::Key;
use crate::rekordbox::{RbLibrary, RbMark, RbPlaylist, RbTrack, path_to_location};
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ORIGIN: crate::rekordbox::Origin = crate::rekordbox::Origin {
    source: "import:virtualdj",
    external: "virtualdj",
    audit_action: "import-virtualdj",
};

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

/// VirtualDJ stores tempo as seconds per beat in its scan data; tags may
/// hold plain BPM. Accept both.
fn bpm_of(v: &str) -> Option<f64> {
    let x: f64 = v.parse().ok()?;
    if x <= 0.0 {
        None
    } else if x < 10.0 {
        Some(60.0 / x)
    } else {
        Some(x)
    }
}

/// Parse `database.xml` into tracks (no playlists).
pub fn parse_database(xml: &str) -> Result<RbLibrary> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut lib = RbLibrary::default();
    let mut saw_root = false;
    let mut cur: Option<RbTrack> = None;
    let mut in_comment = false;
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| Error::Xml(format!("at byte {}: {e}", reader.buffer_position())))?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(ev, Event::Empty(_));
                let a = attrs(e)?;
                match e.name().as_ref() {
                    "VirtualDJ_Database" => saw_root = true,
                    "Song" => {
                        let Some(path) = a.get("FilePath") else {
                            continue;
                        };
                        let loc = path_to_location(path);
                        let mut t = RbTrack {
                            track_id: path.clone(),
                            ..Default::default()
                        };
                        t.attrs.insert("Location".into(), loc);
                        if let Some(s) = a.get("FileSize") {
                            t.attrs.insert("Size".into(), s.clone());
                        }
                        if empty {
                            lib.tracks.push(t);
                        } else {
                            cur = Some(t);
                        }
                    }
                    "Tags" => {
                        if let Some(t) = cur.as_mut() {
                            for (from, to) in [
                                ("Author", "Artist"),
                                ("Title", "Name"),
                                ("Genre", "Genre"),
                                ("Album", "Album"),
                                ("Label", "Label"),
                                ("Remix", "Mix"),
                                ("Year", "Year"),
                            ] {
                                if let Some(v) = a.get(from).filter(|v| !v.is_empty()) {
                                    t.attrs.insert(to.into(), v.clone());
                                }
                            }
                            if let Some(s) = a.get("Stars").and_then(|s| s.parse::<i64>().ok()) {
                                t.attrs
                                    .insert("Rating".into(), (s.clamp(0, 5) * 51).to_string());
                            }
                            if let Some(b) = a.get("Bpm").and_then(|b| bpm_of(b)) {
                                t.attrs.insert("AverageBpm".into(), b.to_string());
                            }
                            if let Some(k) = a.get("Key").and_then(|k| Key::parse(k)) {
                                t.attrs.insert("Tonality".into(), k.name());
                            }
                        }
                    }
                    "Infos" => {
                        if let (Some(t), Some(l)) = (cur.as_mut(), a.get("SongLength")) {
                            t.attrs.insert("TotalTime".into(), l.clone());
                        }
                    }
                    "Scan" => {
                        if let Some(t) = cur.as_mut() {
                            if !t.attrs.contains_key("AverageBpm")
                                && let Some(b) = a.get("Bpm").and_then(|b| bpm_of(b))
                            {
                                t.attrs.insert("AverageBpm".into(), b.to_string());
                            }
                            if !t.attrs.contains_key("Tonality")
                                && let Some(k) = a.get("Key").and_then(|k| Key::parse(k))
                            {
                                t.attrs.insert("Tonality".into(), k.name());
                            }
                        }
                    }
                    "Poi" => {
                        if let Some(t) = cur.as_mut() {
                            let pos = a
                                .get("Pos")
                                .and_then(|p| p.parse::<f64>().ok())
                                .unwrap_or(0.0);
                            match a.get("Type").map(String::as_str) {
                                Some("beatgrid") => {
                                    if let Some(b) = t
                                        .attrs
                                        .get("AverageBpm")
                                        .and_then(|b| b.parse::<f64>().ok())
                                    {
                                        t.tempos.push((pos, b));
                                    }
                                }
                                Some("cue") => t.marks.push(RbMark {
                                    name: a.get("Name").cloned().unwrap_or_default(),
                                    kind: 0,
                                    start: pos,
                                    end: None,
                                    num: a
                                        .get("Num")
                                        .and_then(|n| n.parse::<i64>().ok())
                                        .map(|n| n - 1)
                                        .unwrap_or(-1),
                                    color: None,
                                }),
                                _ => {}
                            }
                        }
                    }
                    "Comment" => in_comment = !empty,
                    _ => {}
                }
            }
            Event::Text(txt) if in_comment => {
                if let Some(t) = cur.as_mut() {
                    let v = txt.xml10_content().into_owned();
                    if !v.trim().is_empty() {
                        t.attrs.insert("Comments".into(), v.trim().to_string());
                    }
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "Song" => {
                    if let Some(t) = cur.take() {
                        lib.tracks.push(t);
                    }
                }
                "Comment" => in_comment = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(Error::Xml("not a VirtualDJ database".into()));
    }
    Ok(lib)
}

/// Paths listed in a `.vdjfolder` playlist.
pub fn parse_vdjfolder(xml: &str) -> Result<Vec<String>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let mut saw_root = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| Error::Xml(format!("at byte {}: {e}", reader.buffer_position())))?
        {
            Event::Start(e) | Event::Empty(e) => match e.name().as_ref() {
                "VirtualFolder" => saw_root = true,
                "song" => {
                    if let Some(p) = attrs(&e)?.remove("path") {
                        out.push(p);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(Error::Xml("not a VirtualDJ playlist".into()));
    }
    Ok(out)
}

/// Read a VirtualDJ home folder: `database.xml` plus every `.vdjfolder`
/// under `Playlists/` (sub-folders become playlist folders).
pub fn read_home(home: &Path) -> Result<RbLibrary> {
    let db = home.join("database.xml");
    let mut lib = if db.exists() {
        parse_database(&String::from_utf8_lossy(&std::fs::read(&db)?))?
    } else {
        RbLibrary::default()
    };
    let known: std::collections::HashSet<String> =
        lib.tracks.iter().map(|t| t.track_id.clone()).collect();
    let mut extra: Vec<RbTrack> = Vec::new();
    let pl_root = home.join("Playlists");
    if pl_root.is_dir() {
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&pl_root)
            .into_iter()
            .flatten()
            .map(|e| e.path().to_path_buf())
            .filter(|p| p.extension().is_some_and(|x| x == "vdjfolder"))
            .collect();
        files.sort();
        for f in files {
            let paths = parse_vdjfolder(&String::from_utf8_lossy(&std::fs::read(&f)?))?;
            let folders: Vec<String> = f
                .parent()
                .and_then(|p| p.strip_prefix(&pl_root).ok())
                .map(|p| p.iter().map(|c| c.to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            for p in &paths {
                if !known.contains(p) && !extra.iter().any(|t| &t.track_id == p) {
                    let mut t = RbTrack {
                        track_id: p.clone(),
                        ..Default::default()
                    };
                    t.attrs.insert("Location".into(), path_to_location(p));
                    extra.push(t);
                }
            }
            lib.playlists.push(RbPlaylist {
                folders,
                name: f
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                keys: paths,
                key_type: 0,
            });
        }
    }
    lib.tracks.extend(extra);
    Ok(lib)
}

pub fn build_vdjfolder(lib: &ExportLibrary, tracks: &[usize]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VirtualFolder>\n");
    for (n, i) in tracks.iter().enumerate() {
        let x = &lib.tracks[*i];
        let t = &x.row;
        s.push_str(&format!(
            " <song path=\"{}\" size=\"{}\" songlength=\"{:.1}\" bpm=\"{}\" key=\"{}\" artist=\"{}\" title=\"{}\" idx=\"{n}\" />\n",
            esc(t.path.as_deref().unwrap_or("")),
            x.size,
            t.duration_ms.unwrap_or(0) as f64 / 1000.0,
            t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default(),
            esc(t.key.as_deref().unwrap_or("")),
            esc(t.artist.as_deref().unwrap_or("")),
            esc(t.title.as_deref().unwrap_or(""))
        ));
    }
    s.push_str("</VirtualFolder>\n");
    s
}

/// Write one `.vdjfolder` per playlist under `dir` (folders → sub-folders).
pub fn export(lib: &ExportLibrary, dir: &Path) -> Result<usize> {
    let clean = |s: &str| s.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let mut n = 0;
    for p in &lib.playlists {
        let mut d = dir.to_path_buf();
        for f in &p.folders {
            d.push(clean(f));
        }
        std::fs::create_dir_all(&d)?;
        crate::export::write_atomic(
            &d.join(format!("{}.vdjfolder", clean(&p.name))),
            build_vdjfolder(lib, &p.tracks).as_bytes(),
        )?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_and_playlist() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<VirtualDJ_Database Version="8.5">
 <Song FilePath="C:\Music\a b.mp3" FileSize="8517376">
  <Tags Author="Artist &amp; Co" Title="Track" Genre="House" Remix="Extended Mix" Year="2019" Stars="4" Key="8A" />
  <Infos SongLength="290.5" Bitrate="320" />
  <Comment>nice one</Comment>
  <Scan Version="801" Bpm="0.468750" Key="Am" Flag="32768" />
  <Poi Pos="0.120" Type="beatgrid" />
  <Poi Name="Drop" Pos="64.5" Num="2" Type="cue" />
 </Song>
 <Song FilePath="/Users/dj/x.flac" FileSize="1" />
</VirtualDJ_Database>"#;
        let lib = parse_database(xml).unwrap();
        assert_eq!(lib.tracks.len(), 2);
        let t = &lib.tracks[0];
        assert_eq!(t.attrs["Artist"], "Artist & Co");
        assert_eq!(t.attrs["Rating"], "204");
        assert_eq!(t.attrs["AverageBpm"], "128");
        assert_eq!(Key::parse(&t.attrs["Tonality"]).unwrap().camelot(), "8A");
        assert_eq!(t.attrs["Comments"], "nice one");
        assert_eq!(t.tempos, vec![(0.12, 128.0)]);
        assert_eq!((t.marks[0].num, t.marks[0].start), (1, 64.5));
        assert_eq!(t.attrs["Location"], "file://localhost/C:/Music/a%20b.mp3");
        let pl = r#"<?xml version="1.0" encoding="UTF-8"?><VirtualFolder><song path="C:\Music\a b.mp3" idx="0" /></VirtualFolder>"#;
        assert_eq!(
            parse_vdjfolder(pl).unwrap(),
            vec!["C:\\Music\\a b.mp3".to_string()]
        );
        assert!(parse_database("<x/>").is_err());
    }
}
