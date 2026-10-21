//! Recipes: saved search + actions, on demand and on import, undoable.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::recipes::{self, Action, Recipe};
use dc_core::{db, history, library, tags};

#[test]
fn recipe_runs_on_demand_and_on_import_and_undoes() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
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
    write_wav(&music.join("one.wav"), &audio, 22050);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let pl = library::create_playlist(&conn, "Inbox", None, "manual", None).unwrap();
    let r = Recipe {
        id: String::new(),
        name: "Inbox".into(),
        query: "genre:none".into(),
        actions: vec![
            Action::SetField {
                field: "comment".into(),
                value: Some("to check".into()),
            },
            Action::AddTag { tag: "New".into() },
            Action::Decision {
                value: Some("maybe".into()),
            },
            Action::AddToPlaylist {
                playlist: pl.clone(),
            },
        ],
        on_import: true,
        last_run: None,
    };
    let id = recipes::save(&conn, &r).unwrap();
    let rep = recipes::run(&conn, &id, None).unwrap();
    assert_eq!((rep.tracks, rep.changes), (1, 4));
    let t = &query_tracks(&conn, &TrackQuery::default()).unwrap().tracks[0];
    assert_eq!(
        (t.comment.as_deref(), t.decision.as_deref()),
        (Some("to check"), Some("maybe"))
    );
    assert_eq!(
        library::playlist_track_ids(&conn, &pl).unwrap(),
        vec![t.id.clone()]
    );
    let new_tag = tags::find(&conn, "New").unwrap().unwrap();
    assert!(
        tags::for_track(&conn, &t.id)
            .unwrap()
            .iter()
            .any(|x| x.tag_id == new_tag)
    );
    // Running again changes nothing.
    assert_eq!(recipes::run(&conn, &id, None).unwrap().changes, 0);
    // One undo reverts the whole run.
    history::undo(&conn).unwrap(); // the no-op run recorded nothing, so this undoes the first
    let t2 = library::get_track(&conn, &t.id).unwrap();
    assert_eq!((t2.comment, t2.decision), (None, None));
    assert!(library::playlist_track_ids(&conn, &pl).unwrap().is_empty());

    // On import: only the newly added track is touched.
    write_wav(&music.join("two.wav"), &audio, 22050);
    let imp = library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    assert_eq!(imp.added_track_ids.len(), 1);
    let ran = recipes::run_on_import(&conn, &imp.added_track_ids).unwrap();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].1.tracks, 1);
    assert_eq!(
        library::playlist_track_ids(&conn, &pl).unwrap(),
        imp.added_track_ids
    );

    // Invalid recipes are refused.
    let mut bad = r.clone();
    bad.actions = vec![Action::SetField {
        field: "nope".into(),
        value: None,
    }];
    assert!(recipes::save(&conn, &bad).is_err());
    bad.actions = vec![];
    assert!(recipes::save(&conn, &bad).is_err());
}
