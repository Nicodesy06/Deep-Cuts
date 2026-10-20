//! Rename/move files from metadata with preview, conflicts and undo.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, fileops, history, library};

fn tone(path: &std::path::Path) {
    let audio = synth(
        &Spec {
            bpm: 120.0,
            tonic: 0,
            minor: false,
            seconds: 2.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    write_wav(path, &audio, 22050);
}

#[test]
fn rename_preview_apply_undo_redo() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("in");
    std::fs::create_dir_all(&music).unwrap();
    tone(&music.join("track01.wav"));
    tone(&music.join("track02.wav"));
    tone(&music.join("noise.wav"));
    // an unrelated file that must never be overwritten
    std::fs::write(music.join("Busy - Name.wav"), b"not ours").unwrap();
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music.clone()], None, &mut |_| {}).unwrap();
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let id = |name: &str| {
        rows.iter()
            .find(|t| t.path.as_deref().unwrap().ends_with(name))
            .unwrap()
            .id
            .clone()
    };
    let (a, b, c) = (id("track01.wav"), id("track02.wav"), id("noise.wav"));
    library::edit_field(&conn, std::slice::from_ref(&a), "artist", Some("Röyksopp")).unwrap();
    library::edit_field(
        &conn,
        std::slice::from_ref(&a),
        "title",
        Some("What Else: Is There?"),
    )
    .unwrap();
    library::edit_field(&conn, std::slice::from_ref(&b), "artist", Some("Busy")).unwrap();
    library::edit_field(&conn, std::slice::from_ref(&b), "title", Some("Name")).unwrap();
    let ids = vec![a.clone(), b.clone(), c.clone()];

    let out = tmp.path().join("sorted");
    std::fs::create_dir_all(&out).unwrap();
    let plan = fileops::plan(&conn, &ids, "{artist} - {title} ({version})", None).unwrap();
    assert_eq!(plan.len(), 3);
    let pa = plan.iter().find(|p| p.track_id == a).unwrap();
    assert!(pa.conflict.is_none(), "{pa:?}");
    assert!(
        pa.to.ends_with("Röyksopp - What Else_ Is There_.wav"),
        "{}",
        pa.to
    );
    let pb = plan.iter().find(|p| p.track_id == b).unwrap();
    assert_eq!(pb.conflict.as_deref(), Some("exists"));
    // the file name of the untagged wav comes from its filename-derived title
    let moved_plan = fileops::plan(&conn, &ids, "{artist} - {title}", Some(&out)).unwrap();
    let rep = fileops::apply(&conn, &moved_plan).unwrap();
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(rep.moved + rep.skipped, 3);
    let new_a = out.join("Röyksopp - What Else_ Is There_.wav");
    assert!(new_a.exists() && !music.join("track01.wav").exists());
    assert!(
        library::track_path(&conn, &a)
            .unwrap()
            .ends_with("Röyksopp - What Else_ Is There_.wav")
    );
    assert_eq!(
        std::fs::read(music.join("Busy - Name.wav")).unwrap(),
        b"not ours"
    );

    // undo moves every file back, redo moves them again
    let undone = history::undo(&conn).unwrap().unwrap();
    assert_eq!(undone.action, "rename-files");
    assert!(music.join("track01.wav").exists() && !new_a.exists());
    assert!(
        library::track_path(&conn, &a)
            .unwrap()
            .ends_with("track01.wav")
    );
    history::redo(&conn).unwrap().unwrap();
    assert!(new_a.exists());

    // a re-import finds everything where the database says it is
    let rep = library::import_paths(&mut conn, &[music, out], None, &mut |_| {}).unwrap();
    assert_eq!(
        (rep.added, rep.relinked, rep.unchanged),
        (0, 0, 3),
        "{rep:?}"
    );

    // pattern errors are reported, not guessed
    assert!(fileops::plan(&conn, &ids, "{nope}", None).is_err());
    assert!(fileops::plan(&conn, &ids, "{artist", None).is_err());
}
