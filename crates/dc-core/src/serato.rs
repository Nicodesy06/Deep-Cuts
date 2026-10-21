//! Serato crates (`_Serato_/Subcrates/*.crate`) import and export.
//!
//! A crate is a sequence of fields: 4-byte ASCII tag, big-endian u32
//! length, payload. Strings are UTF-16BE. Track paths are relative to the
//! root of the drive that holds the `_Serato_` folder. Serato reads BPM, key
//! and other metadata from the files' own tags (see "Write tags to files");
//! Serato's cue/beatgrid tags are not written.

use crate::djlib::ExportLibrary;
use crate::error::{Error, Result};
use crate::rekordbox::{RbLibrary, RbPlaylist, RbTrack, path_to_location};
use std::path::{Path, PathBuf};

pub const ORIGIN: crate::rekordbox::Origin = crate::rekordbox::Origin {
    source: "import:serato",
    external: "serato",
    audit_action: "import-serato",
};

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn from_utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn field(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(tag);
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn fields(b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if i + 8 > b.len() {
            return Err(Error::Invalid("truncated Serato crate".into()));
        }
        let tag = [b[i], b[i + 1], b[i + 2], b[i + 3]];
        let len = u32::from_be_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let end = i + 8 + len;
        if end > b.len() {
            return Err(Error::Invalid("truncated Serato crate".into()));
        }
        out.push((tag, &b[i + 8..end]));
        i = end;
    }
    Ok(out)
}

/// Crate bytes for `paths` (already relative to the drive root, '/'-separated).
pub fn build_crate(paths: &[String]) -> Vec<u8> {
    let mut b = field(b"vrsn", &utf16("1.0/Serato ScratchLive Crate"));
    let mut osrt = field(b"tvcn", &utf16("song"));
    osrt.extend(field(b"brev", &[0]));
    b.extend(field(b"osrt", &osrt));
    for col in ["song", "artist", "bpm", "key", "album", "length", "comment"] {
        let mut c = field(b"tvcn", &utf16(col));
        c.extend(field(b"tvcw", &utf16("0")));
        b.extend(field(b"ovct", &c));
    }
    for p in paths {
        b.extend(field(b"otrk", &field(b"ptrk", &utf16(p))));
    }
    b
}

/// Track paths (relative) listed in a crate.
pub fn parse_crate(bytes: &[u8]) -> Result<Vec<String>> {
    let top = fields(bytes)?;
    if top.first().map(|f| &f.0) != Some(b"vrsn") {
        return Err(Error::Invalid("not a Serato crate".into()));
    }
    let mut out = Vec::new();
    for (tag, payload) in top {
        if &tag == b"otrk" {
            for (t, v) in fields(payload)? {
                if &t == b"ptrk" {
                    out.push(from_utf16(v));
                }
            }
        }
    }
    Ok(out)
}

/// The drive root that a `_Serato_` folder belongs to: "/Volumes/X" for
/// external drives on macOS, "X:\\" on Windows, "/" otherwise.
pub fn volume_root(serato_dir: &Path) -> PathBuf {
    let s = serato_dir.to_string_lossy().replace('\\', "/");
    if s.len() > 2 && s.as_bytes()[1] == b':' {
        return PathBuf::from(format!("{}\\", &s[..2]));
    }
    if let Some(r) = s.strip_prefix("/Volumes/")
        && let Some(name) = r.split('/').next()
    {
        return PathBuf::from(format!("/Volumes/{name}"));
    }
    PathBuf::from("/")
}

/// Path relative to `root` with '/' separators, or None if on another drive.
pub fn relative(path: &str, root: &Path) -> Option<String> {
    let p = path.replace('\\', "/");
    let r = root.to_string_lossy().replace('\\', "/");
    let r = r.trim_end_matches('/');
    let rest = if r.is_empty() {
        Some(p.as_str())
    } else {
        p.strip_prefix(r)
    }?;
    if !(rest.starts_with('/') || r.is_empty()) {
        return None;
    }
    // On macOS the startup disk also holds /Volumes/<other>: those are other drives.
    if r.is_empty() && rest.starts_with("/Volumes/") {
        return None;
    }
    Some(rest.trim_start_matches('/').to_string())
}

fn absolute(rel: &str, root: &Path) -> String {
    let r = root.to_string_lossy();
    if r.ends_with('\\') {
        format!("{r}{}", rel.replace('/', "\\"))
    } else {
        format!("{}/{}", r.trim_end_matches('/'), rel)
    }
}

fn crate_file_name(folders: &[String], name: &str) -> String {
    let clean = |s: &str| {
        s.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
            .replace("%%", "_")
    };
    let mut parts: Vec<String> = folders.iter().map(|f| clean(f)).collect();
    parts.push(clean(name));
    format!("{}.crate", parts.join("%%"))
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrateReport {
    pub crates: usize,
    pub tracks: usize,
    /// Tracks skipped because they are on another drive than the crates.
    pub other_drive: usize,
    pub dir: String,
}

/// Write one crate per playlist into `serato_dir/Subcrates` (existing crates
/// with the same name are replaced; others are left alone).
pub fn export(lib: &ExportLibrary, serato_dir: &Path) -> Result<CrateReport> {
    let sub = serato_dir.join("Subcrates");
    std::fs::create_dir_all(&sub)?;
    let root = volume_root(serato_dir);
    let mut rep = CrateReport {
        dir: sub.to_string_lossy().into_owned(),
        ..Default::default()
    };
    for p in &lib.playlists {
        let mut paths = Vec::new();
        for i in &p.tracks {
            match lib.tracks[*i]
                .row
                .path
                .as_deref()
                .and_then(|x| relative(x, &root))
            {
                Some(r) => paths.push(r),
                None => rep.other_drive += 1,
            }
        }
        rep.tracks += paths.len();
        crate::export::write_atomic(
            &sub.join(crate_file_name(&p.folders, &p.name)),
            &build_crate(&paths),
        )?;
        rep.crates += 1;
    }
    Ok(rep)
}

/// Read every crate under `serato_dir/Subcrates` as playlists.
pub fn read_library(serato_dir: &Path) -> Result<RbLibrary> {
    let sub = serato_dir.join("Subcrates");
    let root = volume_root(serato_dir);
    let mut lib = RbLibrary::default();
    let mut seen = std::collections::HashSet::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&sub)
        .map_err(|e| Error::Invalid(format!("{}: {e}", sub.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "crate"))
        .collect();
    entries.sort();
    for path in entries {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("crate")
            .to_string();
        let mut parts: Vec<String> = stem.split("%%").map(String::from).collect();
        let name = parts.pop().unwrap_or_default();
        let rels = parse_crate(&std::fs::read(&path)?)?;
        let mut keys = Vec::new();
        for rel in rels {
            let abs = absolute(&rel, &root);
            let loc = path_to_location(&abs);
            if seen.insert(loc.clone()) {
                let mut t = RbTrack {
                    track_id: loc.clone(),
                    ..Default::default()
                };
                t.attrs.insert("Location".into(), loc.clone());
                lib.tracks.push(t);
            }
            keys.push(loc);
        }
        lib.playlists.push(RbPlaylist {
            folders: parts,
            name,
            keys,
            key_type: 1,
        });
    }
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_round_trip_and_paths() {
        let paths = vec![
            "Users/dj/Music/Ça va.mp3".to_string(),
            "Music/b.flac".to_string(),
        ];
        let b = build_crate(&paths);
        assert_eq!(&b[..4], b"vrsn");
        assert_eq!(parse_crate(&b).unwrap(), paths);
        assert!(parse_crate(b"nope").is_err());
        assert_eq!(
            volume_root(Path::new("/Volumes/USB/_Serato_")),
            PathBuf::from("/Volumes/USB")
        );
        assert_eq!(
            volume_root(Path::new("/Users/dj/Music/_Serato_")),
            PathBuf::from("/")
        );
        assert_eq!(
            volume_root(Path::new("E:\\_Serato_")),
            PathBuf::from("E:\\")
        );
        assert_eq!(
            relative("/Users/dj/Music/a.mp3", Path::new("/")).as_deref(),
            Some("Users/dj/Music/a.mp3")
        );
        assert_eq!(relative("/Volumes/USB/a.mp3", Path::new("/")), None);
        assert_eq!(
            relative("/Volumes/USB/x/a.mp3", Path::new("/Volumes/USB")).as_deref(),
            Some("x/a.mp3")
        );
        assert_eq!(
            relative("E:\\Music\\a.mp3", Path::new("E:\\")).as_deref(),
            Some("Music/a.mp3")
        );
        assert_eq!(
            absolute("Music/a.mp3", Path::new("E:\\")),
            "E:\\Music\\a.mp3"
        );
        assert_eq!(
            crate_file_name(&["Gigs".into()], "Fri/Sat"),
            "Gigs%%Fri_Sat.crate"
        );
    }
}
