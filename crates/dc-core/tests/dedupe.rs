//! Merging duplicates: nothing is deleted, everything is undoable.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, dedupe, history, library, tags};

fn q(conn: &rusqlite::Connection, query: &str) -> Vec<library::TrackRow> {
    query_tracks(
        conn,
        &TrackQuery {
            query: query.into(),
            ..Default::default()
        },
    )
    .unwrap()
    .tracks
}

#[test]
fn merge_repoints_playlists_moves_files_to_trash_and_undoes() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 3.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    write_wav(&music.join("keep.wav"), &audio, 22050);
    std::fs::copy(music.join("keep.wav"), music.join("copy.wav")).unwrap();
    write_wav(&music.join("other.wav"), &audio[..audio.len() / 2], 22050);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id = |name: &str| {
        q(&conn, "")
            .into_iter()
            .find(|t| t.path.as_deref().unwrap().ends_with(name))
            .unwrap()
            .id
    };
    let (keep, copy, other) = (id("keep.wav"), id("copy.wav"), id("other.wav"));

    // The copy carries user state the kept track lacks.
    library::set_decision(&conn, std::slice::from_ref(&copy), "global", Some("keep")).unwrap();
    let tag = tags::ensure(&conn, "Peak time", "mood").unwrap();
    tags::assign(&conn, std::slice::from_ref(&copy), &tag, true).unwrap();
    let pl = library::create_playlist(&conn, "Friday", None, "manual", None).unwrap();
    library::reorder_playlist(
        &conn,
        &pl,
        &[other.clone(), copy.clone(), keep.clone()],
        "playlist-add",
    )
    .unwrap();
    let pl2 = library::create_playlist(&conn, "Only copy", None, "manual", None).unwrap();
    library::reorder_playlist(&conn, &pl2, std::slice::from_ref(&copy), "playlist-add").unwrap();

    assert!(
        dedupe::suggest_keep(&conn, &[copy.clone(), keep.clone()])
            .unwrap()
            .is_some()
    );
    let trash = tmp.path().join("trash");
    let rep = dedupe::merge(&conn, &keep, std::slice::from_ref(&copy), &trash).unwrap();
    assert_eq!(
        (rep.merged, rep.files_moved, rep.playlists_updated),
        (1, 1, 2),
        "{rep:?}"
    );
    assert!(rep.failed.is_empty());

    // Playlists point at the kept track, once.
    assert_eq!(
        library::playlist_track_ids(&conn, &pl).unwrap(),
        vec![other.clone(), keep.clone()]
    );
    assert_eq!(
        library::playlist_track_ids(&conn, &pl2).unwrap(),
        vec![keep.clone()]
    );
    // State carried over.
    let k = library::get_track(&conn, &keep).unwrap();
    assert_eq!(k.decision.as_deref(), Some("keep"));
    assert!(
        tags::for_track(&conn, &keep)
            .unwrap()
            .iter()
            .any(|t| t.tag_id == tag)
    );
    // The copy is archived (hidden) and its file is in the trash, not deleted.
    assert!(!music.join("copy.wav").exists());
    let trashed: Vec<_> = walk(&trash);
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert!(q(&conn, "").iter().all(|t| t.id != copy));
    assert_eq!(q(&conn, "archived:yes").len(), 1);
    // No more duplicate group for these two.
    assert!(
        library::duplicate_groups(&conn, 50)
            .unwrap()
            .iter()
            .all(|g| !g.tracks.iter().any(|t| t.id == copy))
    );

    // Undo restores files, playlists and visibility.
    let undone = history::undo(&conn).unwrap().unwrap();
    assert_eq!(undone.action, "merge-duplicates");
    assert!(music.join("copy.wav").exists());
    assert!(walk(&trash).is_empty());
    assert_eq!(
        library::playlist_track_ids(&conn, &pl).unwrap(),
        vec![other.clone(), copy.clone(), keep.clone()]
    );
    assert!(q(&conn, "").iter().any(|t| t.id == copy));
    assert_eq!(library::get_track(&conn, &keep).unwrap().decision, None);

    // Unused files: a new file in the folder is reported, library files are not.
    std::fs::copy(music.join("keep.wav"), music.join("new.wav")).unwrap();
    let unused = dedupe::unused_files(&conn, &music).unwrap();
    assert_eq!(unused.len(), 1);
    assert!(unused[0].ends_with("new.wav"));
    std::fs::remove_file(music.join("new.wav")).unwrap();

    // Broken playlist report: delete a file on disk, mark missing.
    std::fs::remove_file(music.join("other.wav")).unwrap();
    library::check_missing_files(&conn).unwrap();
    let broken = dedupe::broken_playlists(&conn).unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(
        (broken[0].name.as_str(), broken[0].missing, broken[0].total),
        ("Friday", 1, 3)
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    if !dir.exists() {
        return vec![];
    }
    walkdir(dir)
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}
