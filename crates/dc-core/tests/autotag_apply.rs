//! Applying auto-tag proposals: one undoable batch, user values win.

mod common;

use common::*;
use dc_core::autotag::{self, Http, SourcedProposal};
use dc_core::{db, history, library};

#[test]
fn proposals_apply_as_one_undoable_step() {
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
    write_wav(&music.join("Bicep - Glue.wav"), &audio, 22050);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id: String = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .unwrap();
    let p = |field: &str, new: &str| SourcedProposal {
        track_id: id.clone(),
        field: field.into(),
        old: String::new(),
        new: new.into(),
        provider: "deezer".into(),
        score: 1.0,
    };
    let props = vec![
        p("label", "Ninja Tune"),
        p("year", "2017"),
        p("isrc", "GBCFB1700229"),
    ];
    let (n, art) = autotag::apply(&mut conn, &Http::new(None), &props, None).unwrap();
    assert_eq!((n, art.written), (3, 0));
    let t = library::get_track(&conn, &id).unwrap();
    assert_eq!(
        (t.label.as_deref(), t.year),
        (Some("Ninja Tune"), Some(2017))
    );
    let isrc: Option<String> = conn
        .query_row("SELECT isrc FROM tracks WHERE id = ?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(isrc.as_deref(), Some("GBCFB1700229"));
    history::undo(&conn).unwrap();
    let t = library::get_track(&conn, &id).unwrap();
    assert_eq!((t.label, t.year), (None, None));
}
