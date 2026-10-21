//! Fault-injection tests for file operations (audit DC-01..04, FS-001..004):
//! a file move is never half-applied between disk and database.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, dedupe, fileops, history, library};
use std::path::Path;

fn tone(path: &Path, bpm: f32) {
    let audio = synth(
        &Spec {
            bpm,
            tonic: 0,
            minor: false,
            seconds: 2.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    write_wav(path, &audio, 22050);
}

fn setup(names: &[&str]) -> (tempfile::TempDir, rusqlite::Connection, Vec<String>) {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("in");
    std::fs::create_dir_all(&music).unwrap();
    for (i, n) in names.iter().enumerate() {
        tone(&music.join(n), 110.0 + i as f32);
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let ids = names
        .iter()
        .map(|n| {
            rows.iter()
                .find(|t| t.path.as_deref().unwrap().ends_with(n))
                .unwrap()
                .id
                .clone()
        })
        .collect();
    (tmp, conn, ids)
}

fn plan_to(conn: &rusqlite::Connection, id: &str, to: &Path) -> fileops::MovePlan {
    let mut p = fileops::plan(conn, &[id.to_string()], "{title}", None)
        .unwrap()
        .remove(0);
    p.to = to.to_string_lossy().into_owned();
    p.conflict = None;
    p
}

/// FS-001: the source vanished and a different file sits at the target:
/// the move is refused, the database and the other file are unchanged.
#[test]
fn missing_source_with_different_target_is_refused() {
    let (tmp, conn, ids) = setup(&["a.wav"]);
    let src = tmp.path().join("in/a.wav");
    let dst = tmp.path().join("in/renamed.wav");
    let plan = plan_to(&conn, &ids[0], &dst);
    std::fs::remove_file(&src).unwrap();
    std::fs::write(&dst, b"someone else's file").unwrap();
    let rep = fileops::apply(&conn, &[plan]).unwrap();
    assert_eq!(rep.moved, 0);
    assert_eq!(rep.failed.len(), 1, "{rep:?}");
    assert_eq!(std::fs::read(&dst).unwrap(), b"someone else's file");
    assert!(
        library::track_path(&conn, &ids[0])
            .unwrap()
            .ends_with("a.wav")
    );
}

/// A repeated move whose source is gone but whose target holds exactly the
/// registered file is accepted (idempotent resume).
#[test]
fn missing_source_with_same_file_at_target_resumes() {
    let (tmp, conn, ids) = setup(&["a.wav"]);
    let src = tmp.path().join("in/a.wav");
    let dst = tmp.path().join("in/renamed.wav");
    let plan = plan_to(&conn, &ids[0], &dst);
    std::fs::rename(&src, &dst).unwrap();
    let rep = fileops::apply(&conn, &[plan]).unwrap();
    assert_eq!(rep.moved, 1, "{rep:?}");
    assert!(
        library::track_path(&conn, &ids[0])
            .unwrap()
            .ends_with("renamed.wav")
    );
}

/// FS-002 / DC-04: in a merge of three duplicates the second move fails:
/// nothing is archived, playlists are untouched and the first file is back
/// in place.
#[test]
fn merge_is_all_or_nothing() {
    let (tmp, conn, ids) = setup(&["keep.wav", "copy1.wav", "copy2.wav"]);
    let pl = library::create_playlist(&conn, "set", None, "manual", None).unwrap();
    library::add_to_playlist(&conn, &pl, &ids).unwrap();
    let before = library::playlist_track_ids(&conn, &pl).unwrap();
    let trash = tmp.path().join("trash");
    fileops::fault::set("move", "copy2.wav");
    let err = dedupe::merge(&conn, &ids[0], &ids[1..], &trash).unwrap_err();
    fileops::fault::clear();
    assert!(err.to_string().contains("merge cancelled"), "{err}");
    for n in ["keep.wav", "copy1.wav", "copy2.wav"] {
        assert!(
            tmp.path().join("in").join(n).exists(),
            "{n} must stay in place"
        );
    }
    assert_eq!(library::playlist_track_ids(&conn, &pl).unwrap(), before);
    let archived: i64 = conn
        .query_row("SELECT count(*) FROM tracks WHERE archived = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(archived, 0);
    for (id, n) in ids.iter().zip(["keep.wav", "copy1.wav", "copy2.wav"]) {
        assert!(library::track_path(&conn, id).unwrap().ends_with(n));
    }
    // No undo entry for the cancelled merge.
    assert!(
        history::recent(&conn, 5)
            .unwrap()
            .iter()
            .all(|b| b.action != "merge-duplicates")
    );
    // And the same merge succeeds once the fault is gone.
    let rep = dedupe::merge(&conn, &ids[0], &ids[1..], &trash).unwrap();
    assert_eq!(rep.files_moved, 2);
}

/// FS-004: the process "stops" after the file moved but before the
/// transaction committed; reopening the library puts the file back where
/// the committed database says it is.
#[test]
fn crash_before_commit_is_recovered_on_open() {
    let (tmp, conn, ids) = setup(&["a.wav"]);
    let src = tmp.path().join("in/a.wav");
    let dst = tmp.path().join("in/moved/a-renamed.wav");
    let plan = plan_to(&conn, &ids[0], &dst);
    fileops::fault::set("crash-after-move", "a.wav");
    let rep = fileops::apply(&conn, &[plan]).unwrap();
    fileops::fault::clear();
    assert_eq!(rep.moved, 0);
    // Simulated crash: the file is at the destination, the database still
    // points to the source, the journal is on disk.
    assert!(dst.exists() && !src.exists());
    // An abandoned partial copy from an interrupted cross-volume move.
    std::fs::write(
        dst.parent().unwrap().join(".deepcuts-partial.test"),
        b"half",
    )
    .unwrap();
    drop(conn);
    let conn = db::open(&tmp.path().join("lib.db")).unwrap();
    assert!(
        src.exists() && !dst.exists(),
        "file must be back at the source"
    );
    assert!(
        !dst.parent()
            .unwrap()
            .join(".deepcuts-partial.test")
            .exists()
    );
    assert!(
        library::track_path(&conn, &ids[0])
            .unwrap()
            .ends_with("a.wav")
    );
    // The journal is empty afterwards.
    let jdir = format!("{}.fileops", tmp.path().join("lib.db").display());
    assert_eq!(std::fs::read_dir(jdir).unwrap().count(), 0);
}

/// DC-01: a failure later in the same transaction reverses moves that
/// already happened.
#[test]
fn rollback_reverses_moves() {
    let (tmp, conn, ids) = setup(&["a.wav", "b.wav"]);
    let a_dst = tmp.path().join("in/a2.wav");
    let b_dst = tmp.path().join("in/b2.wav");
    let plans = [
        plan_to(&conn, &ids[0], &a_dst),
        plan_to(&conn, &ids[1], &b_dst),
    ];
    let err = history::batch(&conn, "test", "two moves", |tx, b| {
        for p in &plans {
            history::change(
                tx,
                b,
                history::Kind::FilePath,
                &p.track_id,
                &p.file_id,
                history::State::set(Some(&p.to)),
            )?;
        }
        Err::<(), _>(dc_core::Error::Invalid("later step failed".into()))
    })
    .unwrap_err();
    assert!(err.to_string().contains("later step failed"));
    assert!(tmp.path().join("in/a.wav").exists() && !a_dst.exists());
    assert!(tmp.path().join("in/b.wav").exists() && !b_dst.exists());
}

/// FS-003 (same-volume part): a move never leaves a truncated file; the
/// destination either holds the full verified file or does not exist.
#[test]
fn moved_file_is_byte_identical() {
    let (tmp, conn, ids) = setup(&["a.wav"]);
    let src = tmp.path().join("in/a.wav");
    let want = fileops::file_hash(&src).unwrap();
    let dst = tmp.path().join("elsewhere/a.wav");
    let rep = fileops::apply(&conn, &[plan_to(&conn, &ids[0], &dst)]).unwrap();
    assert_eq!(rep.moved, 1);
    assert_eq!(fileops::file_hash(&dst).unwrap(), want);
    let partials = std::fs::read_dir(dst.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".deepcuts-partial")
        })
        .count();
    assert_eq!(partials, 0);
}
