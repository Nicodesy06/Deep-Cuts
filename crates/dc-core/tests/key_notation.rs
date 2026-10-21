//! Rewriting the key notation in files: only the key field, verified.

mod common;

use common::*;
use dc_core::tagwrite::{self, KeyNotation, WriteOptions};
use dc_core::{db, library, metadata};

#[test]
fn rewrite_key_notation_in_files_only() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not available: skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    let wav = tmp.path().join("src.wav");
    write_wav(
        &wav,
        &synth(
            &Spec {
                bpm: 124.0,
                tonic: 9,
                minor: true,
                seconds: 3.0,
                first_beat_s: 0.0,
            },
            44100,
        ),
        44100,
    );
    let flac = music.join("a.flac");
    assert!(ffmpeg_encode(
        &wav,
        &flac,
        &["-c:a", "flac"],
        &[
            ("title", "One"),
            ("artist", "Alpha"),
            ("KEY", "Am"),
            ("INITIALKEY", "Am")
        ]
    ));
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id: String = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        metadata::read(&flac).unwrap().musical_key.as_deref(),
        Some("Am")
    );
    // A pending title edit must not be written by the key-only action.
    library::edit_field(
        &conn,
        std::slice::from_ref(&id),
        "title",
        Some("One (edit)"),
    )
    .unwrap();

    let opts = WriteOptions {
        key_notation: KeyNotation::Camelot,
        fields: Some(vec!["key".into()]),
    };
    let plan = tagwrite::plan_with(&conn, std::slice::from_ref(&id), &opts).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].changes.len(), 1, "{:?}", plan[0].changes);
    assert_eq!(plan[0].changes[0].new_value.as_deref(), Some("8A"));
    let rep = tagwrite::write_with(&mut conn, std::slice::from_ref(&id), None, &opts).unwrap();
    assert_eq!((rep.written, rep.failed.len()), (1, 0), "{rep:?}");
    let m = metadata::read(&flac).unwrap();
    assert_eq!(m.musical_key.as_deref(), Some("8A"));
    assert_eq!(m.title.as_deref(), Some("One"));
    // Nothing left to do; Open Key would change it again.
    assert!(
        tagwrite::plan_with(&conn, std::slice::from_ref(&id), &opts)
            .unwrap()
            .is_empty()
    );
    let ok = WriteOptions {
        key_notation: KeyNotation::OpenKey,
        fields: Some(vec!["key".into()]),
    };
    let p = tagwrite::plan_with(&conn, std::slice::from_ref(&id), &ok).unwrap();
    assert_eq!(p[0].changes[0].new_value.as_deref(), Some("1m"));
    assert_eq!(KeyNotation::Musical.format("8A"), "Am");
    assert_eq!(KeyNotation::Original.format("8A"), "8A");
}
