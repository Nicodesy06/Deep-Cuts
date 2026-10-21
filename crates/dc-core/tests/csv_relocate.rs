//! CSV metadata import (with mapping and preview) and library relocation.

mod common;

use common::*;
use dc_core::csvimport::{self, MatchBy};
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{bulk, db, export, history, library, relocate};

fn lib(dir: &std::path::Path) -> (rusqlite::Connection, Vec<library::TrackRow>) {
    let music = dir.join("music");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 2.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    for n in ["Alpha - One.wav", "Beta - Two.wav", "Gamma - Three.wav"] {
        write_wav(&music.join(n), &audio, 22050);
    }
    let mut conn = db::open(&dir.join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    (conn, rows)
}

#[test]
fn csv_import_by_artist_title_and_by_path_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let (conn, rows) = lib(tmp.path());
    let alpha = rows
        .iter()
        .find(|r| r.artist.as_deref() == Some("Alpha"))
        .unwrap()
        .id
        .clone();

    // A spreadsheet from elsewhere: semicolons, other header names, a bad BPM.
    let text = "Track Artist;Track Title;Tempo;Initial Key;Genre;Stars;Decision\n\
                Alpha;One;124,5;8A;Deep House;★★★★;keep\n\
                beta;two;fast;;Techno;;\n\
                Nobody;Nothing;120;;House;;\n";
    let sheet = csvimport::parse(text).unwrap();
    assert_eq!(sheet.delimiter, ';');
    let m = sheet.suggested.clone().unwrap();
    assert_eq!(m.match_by, MatchBy::ArtistTitle);
    let pv = csvimport::preview(&conn, &sheet, &m).unwrap();
    assert_eq!((pv.rows, pv.matched, pv.unmatched), (3, 2, 1));
    assert_eq!(pv.invalid, vec!["BPM \"fast\"".to_string()]);
    let fields: Vec<(&str, Option<&str>)> = pv
        .proposals
        .iter()
        .filter(|p| p.track_id == alpha)
        .map(|p| (p.field.as_str(), p.new.as_deref()))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("bpm", Some("124.5")),
            ("key", Some("8A")),
            ("genre", Some("Deep House")),
            ("rating", Some("4")),
            ("decision", Some("keep"))
        ]
    );
    let n = bulk::apply_as(&conn, "csv-import", &pv.proposals).unwrap();
    assert_eq!(n, pv.proposals.len());
    let a = library::get_track(&conn, &alpha).unwrap();
    assert_eq!(
        (a.bpm, a.camelot.as_deref(), a.rating, a.decision.as_deref()),
        (Some(124.5), Some("8A"), Some(4), Some("keep"))
    );
    // Importing the same file again changes nothing.
    assert!(
        csvimport::preview(&conn, &sheet, &m)
            .unwrap()
            .proposals
            .iter()
            .all(|p| p.track_id != alpha)
    );
    assert_eq!(history::undo(&conn).unwrap().unwrap().action, "csv-import");
    assert_eq!(library::get_track(&conn, &alpha).unwrap().rating, None);

    // Our own CSV export re-imports by path.
    bulk::apply_as(&conn, "csv-import", &pv.proposals).unwrap();
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let out = tmp.path().join("export.csv");
    export::export_csv(&conn, &ids, &out).unwrap();
    history::undo(&conn).unwrap();
    let sheet = csvimport::read(&out).unwrap();
    let m = sheet.suggested.clone().unwrap();
    assert_eq!(m.match_by, MatchBy::Path);
    let pv = csvimport::preview(&conn, &sheet, &m).unwrap();
    assert_eq!(pv.matched, 3);
    assert!(
        pv.proposals
            .iter()
            .any(|p| p.track_id == alpha && p.field == "genre")
    );
}

#[test]
fn relocate_rewrites_paths_and_undoes() {
    let tmp = tempfile::tempdir().unwrap();
    let (conn, _) = lib(tmp.path());
    let old = tmp.path().join("music");
    let new = tmp.path().join("moved");
    // The prefix as stored (canonical), taken while the folder still exists.
    let from = library::path_key(&old);
    std::fs::rename(&old, &new).unwrap();
    library::check_missing_files(&conn).unwrap();
    assert_eq!(
        query_tracks(
            &conn,
            &TrackQuery {
                query: "missing:yes".into(),
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        3
    );

    let pv = relocate::preview(&conn, &from, &new.to_string_lossy()).unwrap();
    assert_eq!((pv.matched, pv.found), (3, 3));
    assert_eq!(
        relocate::apply(&conn, &from, &new.to_string_lossy(), true).unwrap(),
        3
    );
    assert_eq!(
        query_tracks(
            &conn,
            &TrackQuery {
                query: "missing:yes".into(),
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        0
    );
    history::undo(&conn).unwrap();
    assert_eq!(
        query_tracks(
            &conn,
            &TrackQuery {
                query: "missing:yes".into(),
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        3
    );
}
