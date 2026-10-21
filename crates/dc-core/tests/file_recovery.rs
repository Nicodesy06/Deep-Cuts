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

/// DC-06: a tag write that fails before the file is replaced leaves the
/// original bytes untouched and no staged copy behind, also without a
/// backup folder.
#[test]
fn failed_tag_write_keeps_original_bytes() {
    let (tmp, mut conn, ids) = setup(&["a.wav"]);
    let path = tmp.path().join("in/a.wav");
    let before = std::fs::read(&path).unwrap();
    library::edit_field(
        &conn,
        std::slice::from_ref(&ids[0]),
        "artist",
        Some("New Artist"),
    )
    .unwrap();
    fileops::fault::set("tag-write", "a.wav");
    let rep = dc_core::tagwrite::write(&mut conn, std::slice::from_ref(&ids[0]), None).unwrap();
    fileops::fault::clear();
    assert_eq!(rep.written, 0);
    assert_eq!(rep.failed.len(), 1, "{rep:?}");
    assert!(rep.failed[0].error.contains("original file unchanged"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let leftovers = std::fs::read_dir(tmp.path().join("in"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".deepcuts-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

/// DC-07: the file was written but the library refresh failed: the error
/// says so, and reopening the library refreshes it from the file.
#[test]
fn failed_refresh_is_reconciled_on_open() {
    let (tmp, mut conn, ids) = setup(&["a.wav"]);
    library::edit_field(
        &conn,
        std::slice::from_ref(&ids[0]),
        "artist",
        Some("New Artist"),
    )
    .unwrap();
    fileops::fault::set("tag-refresh", "a.wav");
    let rep = dc_core::tagwrite::write(&mut conn, std::slice::from_ref(&ids[0]), None).unwrap();
    fileops::fault::clear();
    assert_eq!(rep.failed.len(), 1, "{rep:?}");
    assert!(
        rep.failed[0].error.contains("next start"),
        "{}",
        rep.failed[0].error
    );
    let path = tmp.path().join("in/a.wav");
    assert_eq!(
        dc_core::metadata::read(&path).unwrap().artist.as_deref(),
        Some("New Artist")
    );
    let stale: i64 = conn
        .query_row(
            "SELECT size FROM files WHERE track_id = ?1",
            [&ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    let conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let (size, hash): (i64, String) = conn
        .query_row(
            "SELECT size, quick_hash FROM files WHERE track_id = ?1",
            [&ids[0]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(size as u64, std::fs::metadata(&path).unwrap().len());
    assert_eq!(hash, library::quick_hash(&path).unwrap());
    assert_ne!(stale, size, "the refresh must have updated the row");
    let jdir = format!("{}.tagwrite", tmp.path().join("lib.db").display());
    assert_eq!(std::fs::read_dir(jdir).unwrap().count(), 0);
}

/// WATCH-001 / DC-10: a watched folder with 300 indexed tracks is renamed:
/// every child is marked missing (not only the folder path), a sibling with
/// a common name prefix is untouched, and re-importing relinks all of them
/// with their decisions.
#[test]
fn renamed_folder_marks_children_and_relinks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("crate");
    let sibling = tmp.path().join("crate2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    let sr = 8000;
    for i in 0..300 {
        // Short, distinct tones (different content per file).
        let f = 200.0 + i as f32 * 3.0;
        let n = sr as usize / 10;
        let s: Vec<f32> = (0..n * 2)
            .map(|k| 0.3 * (2.0 * std::f32::consts::PI * f * (k / 2) as f32 / sr as f32).sin())
            .collect();
        write_wav(&dir.join(format!("t{i:03}.wav")), &s, sr);
    }
    tone(&sibling.join("other.wav"), 120.0);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(
        &mut conn,
        &[dir.clone(), sibling.clone()],
        None,
        &mut |_| {},
    )
    .unwrap();
    let all: Vec<String> = conn
        .prepare("SELECT track_id FROM files")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(all.len(), 301);
    library::set_decision(&conn, &all, "global", Some("keep")).unwrap();

    let renamed = tmp.path().join("crate-renamed");
    std::fs::rename(&dir, &renamed).unwrap();
    let n = library::mark_missing_under(&conn, &dir).unwrap();
    assert_eq!(n, 300);
    let sib_missing: i64 = conn
        .query_row(
            "SELECT missing FROM files WHERE path LIKE '%other.wav'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sib_missing, 0);

    let rep = library::import_paths(&mut conn, std::slice::from_ref(&renamed), None, &mut |_| {})
        .unwrap();
    assert_eq!(rep.relinked, 300, "{rep:?}");
    assert_eq!(rep.added, 0);
    let (missing, keep): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT count(*) FROM files WHERE missing = 1),
                    (SELECT count(*) FROM decisions WHERE decision = 'keep')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((missing, keep), (0, 301));
}

/// A vanished folder reached through a symlink (macOS: /var -> /private/var)
/// still matches the canonical paths stored for its files.
#[cfg(unix)]
#[test]
fn vanished_folder_behind_symlink_matches_stored_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir_all(real.join("crate")).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path().join("alias")).unwrap();
    tone(&real.join("crate/a.wav"), 120.0);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let via_alias = tmp.path().join("alias/crate");
    library::import_paths(
        &mut conn,
        std::slice::from_ref(&via_alias),
        None,
        &mut |_| {},
    )
    .unwrap();
    std::fs::remove_dir_all(real.join("crate")).unwrap();
    assert_eq!(library::mark_missing_under(&conn, &via_alias).unwrap(), 1);
}

/// "Start over" empties the library but never touches files on disk, and
/// the backup taken first restores everything.
#[test]
fn reset_library_keeps_files_and_can_be_restored() {
    let (tmp, mut conn, ids) = setup(&["a.wav", "b.wav"]);
    let pl = library::create_playlist(&conn, "From XML", None, "manual", None).unwrap();
    library::add_to_playlist(&conn, &pl, &ids).unwrap();
    library::set_decision(&conn, &ids, "global", Some("keep")).unwrap();
    let m3u = tmp.path().join("in/list.m3u8");
    std::fs::write(&m3u, "#EXTM3U\na.wav\nb.wav\n").unwrap();
    let before: Vec<Vec<u8>> = ["a.wav", "b.wav", "list.m3u8"]
        .iter()
        .map(|n| std::fs::read(tmp.path().join("in").join(n)).unwrap())
        .collect();
    let backups = tmp.path().join("backups");
    let rep = dc_core::backup::reset_library(&conn, &backups).unwrap();
    assert_eq!((rep.tracks, rep.playlists), (2, 1));
    let n =
        |c: &rusqlite::Connection, sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(n(&conn, "SELECT count(*) FROM tracks"), 0);
    assert_eq!(n(&conn, "SELECT count(*) FROM files"), 0);
    assert_eq!(n(&conn, "SELECT count(*) FROM playlists"), 0);
    assert_eq!(n(&conn, "SELECT count(*) FROM decisions"), 0);
    // Files on disk are untouched.
    for (name, bytes) in ["a.wav", "b.wav", "list.m3u8"].iter().zip(&before) {
        assert_eq!(
            &std::fs::read(tmp.path().join("in").join(name)).unwrap(),
            bytes
        );
    }
    // The backup brings the library back.
    dc_core::backup::restore(&mut conn, Path::new(&rep.backup.path), &backups).unwrap();
    assert_eq!(n(&conn, "SELECT count(*) FROM tracks"), 2);
    assert_eq!(library::playlist_track_ids(&conn, &pl).unwrap(), ids);
}
