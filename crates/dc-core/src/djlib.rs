//! A neutral snapshot of (part of) the library for the DJ-software
//! exporters (Rekordbox XML, Traktor NML, VirtualDJ, Serato crates).

use crate::error::Result;
use crate::library::{self, TrackRow};
use crate::rekordbox::{self, Cue};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct XTrack {
    pub row: TrackRow,
    pub size: i64,
    pub sample_rate: Option<i64>,
    /// (bpm, first beat ms): user grid, then imported, then analysis.
    pub grid: Option<(f64, f64)>,
    /// Cue points and loops (suggestions excluded).
    pub cues: Vec<Cue>,
}

#[derive(Debug, Clone)]
pub struct XPlaylist {
    /// Folder names from the root down to the playlist.
    pub folders: Vec<String>,
    pub name: String,
    /// Indices into `ExportLibrary::tracks`.
    pub tracks: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportLibrary {
    pub tracks: Vec<XTrack>,
    pub playlists: Vec<XPlaylist>,
}

fn load_track(conn: &Connection, id: &str) -> Result<XTrack> {
    let row = library::get_track(conn, id)?;
    let (size, sample_rate): (Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT size, sample_rate FROM files WHERE track_id = ?1 ORDER BY missing, added_at LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((None, None));
    let grid: Option<(f64, f64)> = conn
        .query_row(
            "SELECT bpm, first_beat_ms FROM beatgrids WHERE track_id = ?1
             ORDER BY CASE source WHEN 'user' THEN 0 WHEN 'analysis' THEN 2 ELSE 1 END LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let cues = rekordbox::cues(conn, id)?
        .into_iter()
        .filter(|c| c.kind != "suggestion")
        .collect();
    Ok(XTrack {
        row,
        size: size.unwrap_or(0),
        sample_rate,
        grid,
        cues,
    })
}

struct Builder<'a> {
    conn: &'a Connection,
    lib: ExportLibrary,
    index: HashMap<String, usize>,
}

impl Builder<'_> {
    fn idx(&mut self, id: &str) -> Result<usize> {
        if let Some(i) = self.index.get(id) {
            return Ok(*i);
        }
        let t = load_track(self.conn, id)?;
        self.lib.tracks.push(t);
        let i = self.lib.tracks.len() - 1;
        self.index.insert(id.to_string(), i);
        Ok(i)
    }
}

/// Selected tracks as one playlist named `name`.
pub fn for_ids(conn: &Connection, ids: &[String], name: &str) -> Result<ExportLibrary> {
    let mut b = Builder {
        conn,
        lib: ExportLibrary::default(),
        index: HashMap::new(),
    };
    let mut tracks = Vec::with_capacity(ids.len());
    for id in ids {
        tracks.push(b.idx(id)?);
    }
    b.lib.playlists.push(XPlaylist {
        folders: vec![],
        name: name.to_string(),
        tracks,
    });
    Ok(b.lib)
}

/// The whole library: every live track, every manual playlist and set with
/// its folder path, and smart crates as snapshots under "Smart Crates".
pub fn whole_library(conn: &Connection) -> Result<ExportLibrary> {
    let mut b = Builder {
        conn,
        lib: ExportLibrary::default(),
        index: HashMap::new(),
    };
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM live_tracks ORDER BY artist COLLATE NOCASE, title COLLATE NOCASE",
        )?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    for id in &ids {
        b.idx(id)?;
    }
    let pls: Vec<(String, String, Option<String>, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, parent_id, kind FROM playlists ORDER BY name COLLATE NOCASE",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?
    };
    let by_id: HashMap<&str, (&str, Option<&str>)> = pls
        .iter()
        .map(|(id, name, parent, _)| (id.as_str(), (name.as_str(), parent.as_deref())))
        .collect();
    for (id, name, parent, kind) in &pls {
        let mut folders = Vec::new();
        let mut p = parent.as_deref();
        let mut guard = 0;
        while let Some(pid) = p {
            let Some((n, pp)) = by_id.get(pid) else { break };
            folders.insert(0, n.to_string());
            p = *pp;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        let track_ids: Vec<String> = match kind.as_str() {
            "manual" | "set" => library::playlist_track_ids(conn, id)?,
            "smart" => {
                folders.insert(0, "Smart Crates".into());
                crate::query::query_track_ids(
                    conn,
                    &crate::query::TrackQuery {
                        playlist_id: Some(id.clone()),
                        ..Default::default()
                    },
                )?
            }
            _ => continue,
        };
        let mut tracks = Vec::with_capacity(track_ids.len());
        for t in &track_ids {
            tracks.push(b.idx(t)?);
        }
        b.lib.playlists.push(XPlaylist {
            folders,
            name: name.clone(),
            tracks,
        });
    }
    Ok(b.lib)
}

pub fn esc(s: &str) -> String {
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

// ---------------------------------------------------------------------------
// Entry points used by the app and the CLI
// ---------------------------------------------------------------------------

/// DJ software formats supported for import and export.
pub const APPS: &[&str] = &["rekordbox", "traktor", "serato", "virtualdj"];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub tracks: usize,
    pub playlists: usize,
    /// Files written (crates / playlists / collection).
    pub files: usize,
    /// Tracks left out (e.g. on another drive than the Serato crates).
    pub skipped: usize,
    pub target: String,
}

/// Export to `target`: a file for Rekordbox (.xml) and Traktor (.nml), the
/// `_Serato_` folder for Serato, a playlists folder for VirtualDJ.
pub fn export_to(
    app: &str,
    lib: &ExportLibrary,
    target: &std::path::Path,
) -> Result<ExportSummary> {
    let mut sum = ExportSummary {
        tracks: lib.tracks.len(),
        playlists: lib.playlists.len(),
        files: 1,
        skipped: 0,
        target: target.to_string_lossy().into_owned(),
    };
    match app {
        "rekordbox" => {
            crate::export::write_atomic(target, rekordbox::build_xml_lib(lib).as_bytes())?
        }
        "traktor" => crate::export::write_atomic(
            target,
            crate::traktor::build_nml(lib, &crate::traktor::root_volume_name()).as_bytes(),
        )?,
        "serato" => {
            let r = crate::serato::export(lib, target)?;
            sum.files = r.crates;
            sum.skipped = r.other_drive;
            sum.target = r.dir;
        }
        "virtualdj" => sum.files = crate::vdj::export(lib, target)?,
        other => {
            return Err(crate::Error::Invalid(format!(
                "unknown DJ software '{other}'"
            )));
        }
    }
    Ok(sum)
}

/// Import from `source`: Rekordbox XML file, Traktor NML file, Serato
/// `_Serato_` folder, VirtualDJ home folder (database.xml + Playlists).
pub fn import_from(
    conn: &mut Connection,
    app: &str,
    source: &std::path::Path,
) -> Result<rekordbox::RbImportReport> {
    let read = |p: &std::path::Path| -> Result<String> {
        Ok(String::from_utf8_lossy(&std::fs::read(p)?).into_owned())
    };
    match app {
        "rekordbox" => rekordbox::import_xml(conn, source),
        "traktor" => {
            let lib =
                crate::traktor::parse_nml(&read(source)?, &crate::traktor::root_volume_name())?;
            rekordbox::import_library(conn, &lib, crate::traktor::ORIGIN, source)
        }
        "serato" => {
            let lib = crate::serato::read_library(source)?;
            rekordbox::import_library(conn, &lib, crate::serato::ORIGIN, source)
        }
        "virtualdj" => {
            let lib = crate::vdj::read_home(source)?;
            rekordbox::import_library(conn, &lib, crate::vdj::ORIGIN, source)
        }
        other => Err(crate::Error::Invalid(format!(
            "unknown DJ software '{other}'"
        ))),
    }
}
