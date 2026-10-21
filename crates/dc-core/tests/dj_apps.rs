//! Round trips through every DJ software format: export from one library,
//! import into a fresh one, compare metadata, grid, cues and playlists.

mod common;

use common::*;
use dc_core::cues::{self, NewCue};
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, djlib, library, rekordbox};

fn source_library(dir: &std::path::Path) -> (rusqlite::Connection, std::path::PathBuf) {
    let music = dir.join("music");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 4.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    for n in ["a.wav", "b.wav", "c.wav"] {
        write_wav(&music.join(n), &audio, 22050);
    }
    let mut conn = db::open(&dir.join("src.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let id = |n: &str| {
        rows.iter()
            .find(|r| r.path.as_deref().unwrap().ends_with(n))
            .unwrap()
            .id
            .clone()
    };
    let (a, b, c) = (id("a.wav"), id("b.wav"), id("c.wav"));
    for (field, v) in [
        ("title", "Ça va (Extended Mix)"),
        ("artist", "Röyksopp & Co"),
        ("genre", "Deep House"),
        ("label", "Night <Shift>"),
        ("bpm", "124.5"),
        ("key", "Am"),
        ("rating", "4"),
        ("comment", "warm-up"),
    ] {
        library::edit_field(&conn, std::slice::from_ref(&a), field, Some(v)).unwrap();
    }
    conn.execute(
        "INSERT INTO beatgrids (track_id, source, bpm, first_beat_ms) VALUES (?1, 'user', 124.5, 120.0)",
        [&a],
    )
    .unwrap();
    cues::add(
        &conn,
        &a,
        &NewCue {
            kind: "hotcue",
            slot: Some(1),
            start_ms: 1500.0,
            end_ms: None,
            name: Some("Drop"),
            color: None,
        },
    )
    .unwrap();
    cues::add(
        &conn,
        &a,
        &NewCue {
            kind: "loop",
            slot: None,
            start_ms: 2000.0,
            end_ms: Some(3000.0),
            name: Some("Loop"),
            color: None,
        },
    )
    .unwrap();
    let folder = library::create_playlist(&conn, "Gigs", None, "folder", None).unwrap();
    let pl = library::create_playlist(&conn, "Friday", Some(&folder), "manual", None).unwrap();
    library::set_playlist_tracks(&conn, &pl, &[b.clone(), a.clone()]).unwrap();
    let pl2 = library::create_playlist(&conn, "Solo", None, "manual", None).unwrap();
    library::set_playlist_tracks(&conn, &pl2, std::slice::from_ref(&c)).unwrap();
    (conn, music)
}

fn check_import(
    app: &str,
    target: &std::path::Path,
    dir: &std::path::Path,
    music: &std::path::Path,
    full_metadata: bool,
) {
    let mut conn = db::open(&dir.join(format!("dst-{app}.db"))).unwrap();
    let rep = djlib::import_from(&mut conn, app, target).unwrap();
    assert_eq!(rep.missing_files, 0, "{app}: {rep:?}");
    assert!(rep.unresolved_playlist_entries == 0, "{app}: {rep:?}");
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    assert_eq!(rows.len(), 3, "{app}");
    let a = rows
        .iter()
        .find(|r| r.path.as_deref().unwrap() == library::path_key(&music.join("a.wav")))
        .unwrap();
    if full_metadata {
        assert_eq!(a.title.as_deref(), Some("Ça va (Extended Mix)"), "{app}");
        assert_eq!(a.artist.as_deref(), Some("Röyksopp & Co"), "{app}");
        assert_eq!(a.genre.as_deref(), Some("Deep House"), "{app}");
        assert_eq!(a.bpm, Some(124.5), "{app}");
        assert_eq!(a.camelot.as_deref(), Some("8A"), "{app}");
        assert_eq!(a.rating, Some(4), "{app}");
        let cues = rekordbox::cues(&conn, &a.id).unwrap();
        let hot = cues.iter().find(|c| c.kind == "hotcue").unwrap();
        assert_eq!(
            (hot.slot, hot.start_ms.round(), hot.name.as_deref()),
            (Some(1), 1500.0, Some("Drop")),
            "{app}"
        );
        if app != "virtualdj" {
            let lp = cues.iter().find(|c| c.kind == "loop").unwrap();
            assert_eq!(
                (lp.start_ms.round(), lp.end_ms.map(f64::round)),
                (2000.0, Some(3000.0)),
                "{app}"
            );
        }
        let grid: (f64, f64) = conn
            .query_row("SELECT bpm, first_beat_ms FROM beatgrids WHERE track_id = ?1 AND source LIKE 'import:%'", [&a.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((grid.0, grid.1.round()), (124.5, 120.0), "{app}");
    }
    let pls = library::list_playlists(&conn).unwrap();
    let friday = pls
        .iter()
        .find(|p| p.name == "Friday")
        .unwrap_or_else(|| panic!("{app}: {pls:?}"));
    let ids = library::playlist_track_ids(&conn, &friday.id).unwrap();
    let names: Vec<String> = ids
        .iter()
        .map(|i| library::get_track(&conn, i).unwrap().path.unwrap())
        .map(|p| {
            std::path::Path::new(&p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, vec!["b.wav", "a.wav"], "{app}: order kept");
    let parent = friday
        .parent_id
        .as_deref()
        .map(|p| pls.iter().find(|x| x.id == p).unwrap().name.clone());
    assert_eq!(parent.as_deref(), Some("Gigs"), "{app}: folder kept");
    assert!(pls.iter().any(|p| p.name == "Solo"), "{app}");
}

#[test]
fn round_trip_every_dj_format() {
    let tmp = tempfile::tempdir().unwrap();
    let (conn, music) = source_library(tmp.path());
    let lib = djlib::whole_library(&conn).unwrap();
    assert_eq!(lib.tracks.len(), 3);

    let rb = tmp.path().join("export.xml");
    let s = djlib::export_to("rekordbox", &lib, &rb).unwrap();
    assert_eq!((s.tracks, s.playlists), (3, 2));
    check_import("rekordbox", &rb, tmp.path(), &music, true);

    let nml = tmp.path().join("export.nml");
    djlib::export_to("traktor", &lib, &nml).unwrap();
    check_import("traktor", &nml, tmp.path(), &music, true);

    // Serato: crates only carry track lists (metadata lives in file tags).
    let serato = tmp.path().join("_Serato_");
    let s = djlib::export_to("serato", &lib, &serato).unwrap();
    assert_eq!((s.files, s.skipped), (2, 0));
    assert!(serato.join("Subcrates").join("Gigs%%Friday.crate").exists());
    check_import("serato", &serato, tmp.path(), &music, false);

    // VirtualDJ: playlists as .vdjfolder (+ a database.xml written by hand
    // the way VirtualDJ stores metadata).
    let home = tmp.path().join("VirtualDJ");
    djlib::export_to("virtualdj", &lib, &home.join("Playlists")).unwrap();
    assert!(
        home.join("Playlists")
            .join("Gigs")
            .join("Friday.vdjfolder")
            .exists()
    );
    let a_path = library::path_key(&music.join("a.wav"));
    std::fs::write(
        home.join("database.xml"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<VirtualDJ_Database Version="8.5">
 <Song FilePath="{a_path}" FileSize="1">
  <Tags Author="Röyksopp &amp; Co" Title="Ça va (Extended Mix)" Genre="Deep House" Stars="4" Bpm="124.5" Key="Am" />
  <Poi Pos="0.120" Type="beatgrid" />
  <Poi Name="Drop" Pos="1.5" Num="2" Type="cue" />
 </Song>
</VirtualDJ_Database>"#
        ),
    )
    .unwrap();
    check_import("virtualdj", &home, tmp.path(), &music, true);
}
