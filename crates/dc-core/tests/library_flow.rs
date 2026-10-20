//! End-to-end library workflow on real (synthesised) audio files.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{analysis, backup, db, export, jobs, library, rekordbox};
use std::path::PathBuf;

fn spec(bpm: f32, tonic: u8, minor: bool) -> Spec {
    Spec {
        bpm,
        tonic,
        minor,
        seconds: 40.0,
        first_beat_s: 0.25,
    }
}

fn run_all_jobs(conn: &rusqlite::Connection) -> Vec<(Option<String>, Result<(), String>)> {
    let mut out = vec![];
    while let Some((_, track, res)) = jobs::run_one(conn, &|| false).unwrap() {
        out.push((track, res));
    }
    out
}

fn all(conn: &rusqlite::Connection, q: &str) -> Vec<library::TrackRow> {
    query_tracks(
        conn,
        &TrackQuery {
            query: q.into(),
            ..Default::default()
        },
    )
    .unwrap()
    .tracks
}

#[test]
fn import_decide_search_export_restart_analyze() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(music.join("sub")).unwrap();
    make_track(
        &music,
        "Alpha Artist - Rolling Groove.wav",
        &spec(124.0, 9, true),
    ); // A minor
    make_track(
        &music.join("sub"),
        "Beta - Sunrise (Extended Mix).wav",
        &spec(128.0, 0, false),
    ); // C major
    std::fs::write(music.join("notes.txt"), "not audio").unwrap();
    std::fs::write(music.join("broken.mp3"), b"this is not an mp3").unwrap();
    let db_path = tmp.path().join("lib.db");

    // 1. import
    let mut conn = db::open(&db_path).unwrap();
    let rep =
        library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    assert_eq!(rep.scanned, 3, "txt ignored, broken mp3 scanned");
    assert_eq!(rep.added, 2);
    assert_eq!(
        rep.failed.len(),
        1,
        "broken mp3 is reported, not fatal: {:?}",
        rep.failed
    );

    // 2. metadata from file names (WAVs have no tags)
    let tracks = all(&conn, "");
    assert_eq!(tracks.len(), 2);
    let alpha = tracks
        .iter()
        .find(|t| t.artist.as_deref() == Some("Alpha Artist"))
        .unwrap()
        .clone();
    assert_eq!(alpha.title.as_deref(), Some("Rolling Groove"));
    let beta = tracks
        .iter()
        .find(|t| t.artist.as_deref() == Some("Beta"))
        .unwrap()
        .clone();
    assert_eq!(beta.version.as_deref(), Some("Extended Mix"));
    assert!((beta.duration_ms.unwrap() - 40_000).abs() < 50);
    assert_eq!(beta.format.as_deref(), Some("wav"));

    // 3. decisions + search
    library::set_decision(
        &conn,
        std::slice::from_ref(&alpha.id),
        "global",
        Some("keep"),
    )
    .unwrap();
    library::set_decision(
        &conn,
        std::slice::from_ref(&beta.id),
        "global",
        Some("pass"),
    )
    .unwrap();
    library::set_decision(
        &conn,
        std::slice::from_ref(&beta.id),
        "warmup-profile",
        Some("keep"),
    )
    .unwrap();
    assert_eq!(all(&conn, "rolling").len(), 1);
    assert_eq!(all(&conn, "roll").len(), 1, "prefix match");
    assert_eq!(all(&conn, "decision:keep")[0].id, alpha.id);
    assert_eq!(all(&conn, "-decision:pass").len(), 1);
    assert_eq!(all(&conn, "artist:alpha OR artist:beta").len(), 2);
    assert_eq!(all(&conn, "version:\"Extended Mix\"")[0].id, beta.id);
    assert!(all(&conn, "decision:maybe").is_empty());
    assert!(
        library::set_decision(
            &conn,
            std::slice::from_ref(&alpha.id),
            "global",
            Some("love")
        )
        .is_err()
    );

    // 4. playlist + export
    let pl = library::create_playlist(&conn, "Friday", None, "manual", None).unwrap();
    library::add_to_playlist(&conn, &pl, &[beta.id.clone(), alpha.id.clone()]).unwrap();
    let ids = library::playlist_track_ids(&conn, &pl).unwrap();
    assert_eq!(ids, vec![beta.id.clone(), alpha.id.clone()]);
    let m3u = tmp.path().join("friday.m3u8");
    assert_eq!(export::export_m3u8(&conn, &ids, &m3u).unwrap(), 2);
    let text = std::fs::read_to_string(&m3u).unwrap();
    assert!(
        text.starts_with("#EXTM3U\n#EXTINF:40,Beta - Sunrise (Extended Mix)\n"),
        "{text}"
    );
    assert!(text.contains("Rolling Groove.wav"));
    export::export_csv(&conn, &ids, &tmp.path().join("friday.csv")).unwrap();

    // 5. restart: everything persists
    drop(conn);
    let mut conn = db::open(&db_path).unwrap();
    let a = library::get_track(&conn, &alpha.id).unwrap();
    assert_eq!(a.decision.as_deref(), Some("keep"));
    assert_eq!(
        library::get_track(&conn, &beta.id)
            .unwrap()
            .decision
            .as_deref(),
        Some("pass")
    );
    assert_eq!(library::playlist_track_ids(&conn, &pl).unwrap().len(), 2);

    // 6. analysis job: authentic DSP on the decoded audio
    let q = jobs::enqueue_analysis(&conn, &[alpha.id.clone(), beta.id.clone()], false).unwrap();
    assert_eq!(q.queued, 2);
    assert_eq!(
        jobs::enqueue_analysis(&conn, std::slice::from_ref(&alpha.id), false)
            .unwrap()
            .already_queued,
        1
    );
    let results = run_all_jobs(&conn);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.1.is_ok()), "{results:?}");
    let ra = analysis::latest_result(&conn, &alpha.id).unwrap().unwrap();
    let rb = analysis::latest_result(&conn, &beta.id).unwrap().unwrap();
    let ta = ra.tempo.as_ref().unwrap();
    assert!((ta.bpm - 124.0).abs() < 0.1, "alpha bpm {}", ta.bpm);
    assert!(
        (rb.tempo.as_ref().unwrap().bpm - 128.0).abs() < 0.1,
        "beta bpm {:?}",
        rb.tempo
    );
    assert_eq!(ra.key.as_ref().unwrap().camelot, "8A", "{:?}", ra.key);
    assert_eq!(rb.key.as_ref().unwrap().camelot, "8B", "{:?}", rb.key);
    assert!(
        (ta.first_beat_ms - 250.0).abs() < 25.0,
        "first beat {}",
        ta.first_beat_ms
    );
    assert!(ra.loudness.integrated_lufs.unwrap() < -3.0);
    assert!(
        analysis::waveform(&conn, &alpha.id).unwrap().unwrap().len() == analysis::WAVEFORM_BINS * 4
    );
    // analysis values became effective (no file BPM/key existed)
    let a = library::get_track(&conn, &alpha.id).unwrap();
    assert!((a.bpm.unwrap() - 124.0).abs() < 0.1);
    assert_eq!(a.camelot.as_deref(), Some("8A"));
    assert_eq!(a.analysis_status, "done");
    assert_eq!(all(&conn, "bpm:123-125 key:8A").len(), 1);
    assert_eq!(all(&conn, "key:9A+").len(), 1, "8A is compatible with 9A");
    assert_eq!(all(&conn, "analyzed:yes").len(), 2);
    // already analysed => skipped unless forced
    assert_eq!(
        jobs::enqueue_analysis(&conn, std::slice::from_ref(&alpha.id), false)
            .unwrap()
            .skipped_up_to_date,
        1
    );

    // 7. user edits win over analysis and survive re-analysis
    library::edit_field(&conn, std::slice::from_ref(&alpha.id), "bpm", Some("123.5")).unwrap();
    jobs::enqueue_analysis(&conn, std::slice::from_ref(&alpha.id), true).unwrap();
    run_all_jobs(&conn);
    assert_eq!(
        library::get_track(&conn, &alpha.id).unwrap().bpm,
        Some(123.5)
    );
    // ...and can be reverted to the measured value
    library::clear_field_source(&conn, &alpha.id, "bpm", "user").unwrap();
    assert!((library::get_track(&conn, &alpha.id).unwrap().bpm.unwrap() - 124.0).abs() < 0.1);

    // 8. incremental re-import: nothing changes
    let rep =
        library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    assert_eq!((rep.added, rep.unchanged, rep.updated), (0, 2, 0));

    // 9. move a file: identity, decisions and playlist membership survive
    let moved = tmp.path().join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::rename(
        music.join("Alpha Artist - Rolling Groove.wav"),
        moved.join("renamed.wav"),
    )
    .unwrap();
    let (_, missing) = library::check_missing_files(&conn).unwrap();
    assert_eq!(missing, 1);
    assert_eq!(all(&conn, "missing:yes").len(), 1);
    let rep =
        library::import_paths(&mut conn, std::slice::from_ref(&moved), None, &mut |_| {}).unwrap();
    assert_eq!(rep.relinked, 1, "{rep:?}");
    assert_eq!(rep.added, 0);
    let a = library::get_track(&conn, &alpha.id).unwrap();
    assert!(a.path.unwrap().ends_with("renamed.wav"));
    assert_eq!(a.decision.as_deref(), Some("keep"));
    assert!(!a.missing);
    assert_eq!(all(&conn, "").len(), 2);
    assert!(
        library::playlist_track_ids(&conn, &pl)
            .unwrap()
            .contains(&alpha.id)
    );

    // 10. context decisions are separate from global
    let ctx: String = conn
        .query_row(
            "SELECT decision FROM decisions WHERE track_id = ?1 AND context = 'warmup-profile'",
            [&beta.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ctx, "keep");

    // 11. smart playlist
    let smart = library::create_playlist(
        &conn,
        "Keepers",
        None,
        "smart",
        Some("decision:keep bpm:>120"),
    )
    .unwrap();
    let page = query_tracks(
        &conn,
        &TrackQuery {
            playlist_id: Some(smart),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.total, 1);
    assert!(library::create_playlist(&conn, "bad", None, "smart", Some("bpm:abc")).is_err());
}

#[test]
fn backup_and_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    make_track(
        &music,
        "A - One.wav",
        &Spec {
            bpm: 120.0,
            tonic: 2,
            minor: true,
            seconds: 10.0,
            first_beat_s: 0.0,
        },
    );
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let id = all(&conn, "")[0].id.clone();
    library::set_decision(&conn, std::slice::from_ref(&id), "global", Some("keep")).unwrap();
    let bdir = tmp.path().join("backups");
    let b = backup::create(&conn, &bdir, "manual").unwrap();
    library::set_decision(&conn, std::slice::from_ref(&id), "global", Some("pass")).unwrap();
    library::edit_field(&conn, std::slice::from_ref(&id), "title", Some("Changed")).unwrap();
    let safety = backup::restore(&mut conn, std::path::Path::new(&b.path), &bdir).unwrap();
    let t = library::get_track(&conn, &id).unwrap();
    assert_eq!(t.decision.as_deref(), Some("keep"));
    assert_eq!(t.title.as_deref(), Some("One"));
    let list = backup::list(&bdir).unwrap();
    assert_eq!(list.len(), 2);
    assert!(
        list.iter()
            .any(|x| x.reason == "before-restore" && x.path == safety.path)
    );
    assert_eq!(db::integrity_check(&conn).unwrap(), vec!["ok".to_string()]);
}

#[test]
fn rekordbox_xml_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    make_track(
        &music,
        "Gamma - Deep & Dark.wav",
        &Spec {
            bpm: 122.0,
            tonic: 4,
            minor: true,
            seconds: 12.0,
            first_beat_s: 0.1,
        },
    );
    let mut conn = db::open(&tmp.path().join("a.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id = all(&conn, "")[0].id.clone();
    library::edit_field(&conn, std::slice::from_ref(&id), "rating", Some("4")).unwrap();
    library::edit_field(&conn, std::slice::from_ref(&id), "bpm", Some("122")).unwrap();
    library::edit_field(&conn, std::slice::from_ref(&id), "key", Some("Em")).unwrap();
    library::edit_field(
        &conn,
        std::slice::from_ref(&id),
        "genre",
        Some("Deep House"),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cue_points (id, track_id, kind, slot, start_ms, name, color, source, created_at) VALUES ('c1', ?1, 'hotcue', 0, 1500, 'Intro', '#ff8800', 'user', 0)",
        [&id],
    )
    .unwrap();
    let xml_path = tmp.path().join("rb.xml");
    export::export_rekordbox_xml(
        &conn,
        std::slice::from_ref(&id),
        "Deep Cuts Export",
        &xml_path,
    )
    .unwrap();

    // import into a fresh library
    let mut conn2 = db::open(&tmp.path().join("b.db")).unwrap();
    let rep = rekordbox::import_xml(&mut conn2, &xml_path).unwrap();
    assert_eq!(rep.tracks_in_xml, 1);
    assert_eq!(rep.missing_files, 0);
    assert_eq!(rep.cues_imported, 1);
    assert_eq!(rep.playlists_imported, 1);
    let t = all(&conn2, "")[0].clone();
    assert_eq!(t.title.as_deref(), Some("Deep & Dark"));
    assert_eq!(t.rating, Some(4));
    assert_eq!(t.bpm, Some(122.0));
    assert_eq!(t.camelot.as_deref(), Some("9A"));
    assert_eq!(t.genre.as_deref(), Some("Deep House"));
    let cues = rekordbox::cues(&conn2, &t.id).unwrap();
    assert_eq!(cues[0].start_ms, 1500.0);
    assert_eq!(cues[0].slot, Some(0));
    assert_eq!(cues[0].color.as_deref(), Some("#ff8800"));
    // re-import is idempotent
    let rep2 = rekordbox::import_xml(&mut conn2, &xml_path).unwrap();
    assert_eq!(rep2.matched_existing, 1);
    assert_eq!(all(&conn2, "").len(), 1);
    assert_eq!(library::list_playlists(&conn2).unwrap().len(), 1);
    assert_eq!(rekordbox::cues(&conn2, &t.id).unwrap().len(), 1);
    // user edits made after import beat later re-imports
    library::edit_field(
        &conn2,
        std::slice::from_ref(&t.id),
        "genre",
        Some("Minimal"),
    )
    .unwrap();
    rekordbox::import_xml(&mut conn2, &xml_path).unwrap();
    assert_eq!(
        library::get_track(&conn2, &t.id).unwrap().genre.as_deref(),
        Some("Minimal")
    );

    // an XML whose files are not on this machine keeps them as missing tracks
    let xml = std::fs::read_to_string(&xml_path)
        .unwrap()
        .replace("Deep%20%26%20Dark.wav", "Elsewhere.wav");
    let other = tmp.path().join("other.xml");
    std::fs::write(&other, xml).unwrap();
    let mut conn3 = db::open(&tmp.path().join("c.db")).unwrap();
    let rep3 = rekordbox::import_xml(&mut conn3, &other).unwrap();
    assert_eq!(rep3.missing_files, 1, "{rep3:?}");
    assert_eq!(all(&conn3, "missing:yes").len(), 1);
}

#[test]
fn encoded_formats_with_tags() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not available: skipping multi-format test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let wav = make_track(
        tmp.path(),
        "src.wav",
        &Spec {
            bpm: 126.0,
            tonic: 7,
            minor: true,
            seconds: 30.0,
            first_beat_s: 0.0,
        },
    );
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let tags = [
        ("title", "Night Drive (Dub Mix)"),
        ("artist", "Delta"),
        ("album", "EP1"),
        ("genre", "Tech House"),
        ("TBPM", "126"),
        ("date", "2021"),
    ];
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "t.mp3",
            vec!["-c:a", "libmp3lame", "-b:a", "320k", "-id3v2_version", "3"],
        ),
        ("t.flac", vec!["-c:a", "flac"]),
        ("t.aiff", vec!["-c:a", "pcm_s16be", "-write_id3v2", "1"]),
        ("t.m4a", vec!["-c:a", "aac", "-b:a", "256k"]),
        ("t-alac.m4a", vec!["-c:a", "alac"]),
    ];
    let mut made: Vec<PathBuf> = vec![];
    for (name, args) in &cases {
        let out = music.join(name);
        if ffmpeg_encode(&wav, &out, args, &tags) {
            made.push(out);
        } else {
            eprintln!("ffmpeg could not encode {name}; skipping it");
        }
    }
    assert!(!made.is_empty());
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let rep = library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    assert_eq!(rep.added, made.len(), "{:?}", rep.failed);
    let tracks = all(&conn, "");
    for t in &tracks {
        assert_eq!(t.artist.as_deref(), Some("Delta"), "{:?}", t.path);
        assert_eq!(
            t.title.as_deref(),
            Some("Night Drive (Dub Mix)"),
            "{:?}",
            t.path
        );
        assert_eq!(t.version.as_deref(), Some("Dub Mix"));
        assert_eq!(t.genre.as_deref(), Some("Tech House"), "{:?}", t.path);
    }
    let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    for (track, res) in run_all_jobs(&conn) {
        let t = library::get_track(&conn, track.as_deref().unwrap()).unwrap();
        assert!(res.is_ok(), "{:?}: {res:?}", t.path);
        let r = analysis::latest_result(&conn, &t.id).unwrap().unwrap();
        let bpm = r.tempo.unwrap().bpm;
        assert!((bpm - 126.0).abs() < 0.15, "{:?}: {bpm}", t.path);
        assert_eq!(r.key.unwrap().camelot, "6A", "{:?}", t.path); // G minor
    }
}

#[test]
fn duplicate_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(music.join("copy")).unwrap();
    let s = Spec {
        bpm: 120.0,
        tonic: 0,
        minor: false,
        seconds: 6.0,
        first_beat_s: 0.0,
    };
    let a = make_track(&music, "Echo - Same.wav", &s);
    std::fs::copy(&a, music.join("copy").join("Echo - Same.wav")).unwrap();
    make_track(&music, "Echo - Other.wav", &Spec { bpm: 125.0, ..s });
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let rep = library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    assert_eq!(
        rep.added, 3,
        "a copy at another path is a separate entry, never merged silently"
    );
    let groups = library::duplicate_groups(&conn, 50).unwrap();
    let identical: Vec<_> = groups
        .iter()
        .filter(|g| g.kind == "identical-file")
        .collect();
    assert_eq!(identical.len(), 1);
    assert_eq!(identical[0].tracks.len(), 2);
    assert!(
        groups
            .iter()
            .any(|g| g.kind == "same-artist-title" && g.tracks.len() == 2)
    );
    assert_eq!(all(&conn, "").len(), 3, "nothing removed");
}

#[test]
fn interrupted_jobs_are_recovered_once() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    make_track(
        &music,
        "A - One.wav",
        &Spec {
            bpm: 120.0,
            tonic: 0,
            minor: false,
            seconds: 9.0,
            first_beat_s: 0.0,
        },
    );
    let db_path = tmp.path().join("lib.db");
    let mut conn = db::open(&db_path).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let id = all(&conn, "")[0].id.clone();
    jobs::enqueue_analysis(&conn, &[id], false).unwrap();
    // A job claimed by a process that died...
    jobs::claim_next(&conn).unwrap().unwrap();
    conn.execute("UPDATE jobs SET owner = 'dead-process'", [])
        .unwrap();
    // ...opening more connections (e.g. worker threads) must not touch it,
    let other = db::open(&db_path).unwrap();
    assert_eq!(jobs::summary(&other).unwrap().running, 1);
    // ...but explicit startup recovery re-queues it, and only foreign jobs.
    assert_eq!(jobs::recover_interrupted(&other).unwrap(), 1);
    jobs::claim_next(&other).unwrap().unwrap();
    assert_eq!(
        jobs::recover_interrupted(&other).unwrap(),
        0,
        "own running jobs are left alone"
    );
}
