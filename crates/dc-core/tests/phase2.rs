//! Phase 2: undo/redo, tags, smart fixes, cue suggestions, tag writing,
//! watch folders — on real (synthesised) audio files.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{analysis, cues, db, fixes, history, jobs, library, tags, tagwrite};
use std::path::Path;

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

fn small_library(dir: &Path, names: &[&str]) -> rusqlite::Connection {
    let music = dir.join("m");
    std::fs::create_dir_all(&music).unwrap();
    for n in names {
        make_track(
            &music,
            n,
            &Spec {
                bpm: 124.0,
                tonic: 9,
                minor: true,
                seconds: 8.0,
                first_beat_s: 0.0,
            },
        );
    }
    let mut conn = db::open(&dir.join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    conn
}

#[test]
fn undo_redo_edits_decisions_notes_favorites() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = small_library(tmp.path(), &["A - One.wav", "B - Two.wav"]);
    let ids: Vec<String> = all(&conn, "").iter().map(|t| t.id.clone()).collect();
    let one = all(&conn, "artist:A")[0].id.clone();

    library::edit_field(&conn, &ids, "genre", Some("Minimal")).unwrap();
    library::set_decision(&conn, std::slice::from_ref(&one), "global", Some("keep")).unwrap();
    library::set_notes(&conn, &one, Some("opener")).unwrap();
    library::set_favorite(&conn, std::slice::from_ref(&one), true).unwrap();
    assert_eq!(history::recent(&conn, 10).unwrap().len(), 4);

    // undo in reverse order
    assert_eq!(history::undo(&conn).unwrap().unwrap().action, "favorite");
    assert!(!library::get_track(&conn, &one).unwrap().favorite);
    assert_eq!(history::undo(&conn).unwrap().unwrap().action, "notes");
    assert_eq!(library::track_notes(&conn, &one).unwrap(), None);
    assert_eq!(history::undo(&conn).unwrap().unwrap().action, "decision");
    assert_eq!(library::get_track(&conn, &one).unwrap().decision, None);
    let b = history::undo(&conn).unwrap().unwrap();
    assert_eq!((b.action.as_str(), b.changes), ("edit", 2));
    assert!(all(&conn, "genre:none").len() == 2, "back to no genre");
    assert!(history::undo(&conn).unwrap().is_none());

    // redo re-applies in order
    history::redo(&conn).unwrap().unwrap();
    assert_eq!(all(&conn, "genre:Minimal").len(), 2);
    history::redo(&conn).unwrap().unwrap();
    assert_eq!(
        library::get_track(&conn, &one).unwrap().decision.as_deref(),
        Some("keep")
    );

    // a new action clears the redo stack
    library::edit_field(&conn, std::slice::from_ref(&one), "label", Some("X")).unwrap();
    assert!(history::redo(&conn).unwrap().is_none());

    // no-op edits are not recorded
    let before = history::recent(&conn, 50).unwrap().len();
    library::edit_field(&conn, std::slice::from_ref(&one), "label", Some("X")).unwrap();
    assert_eq!(history::recent(&conn, 50).unwrap().len(), before);

    // undo a user edit that replaced a file value restores the file value
    library::edit_field(&conn, std::slice::from_ref(&one), "title", Some("Renamed")).unwrap();
    history::undo(&conn).unwrap();
    assert_eq!(
        library::get_track(&conn, &one).unwrap().title.as_deref(),
        Some("One")
    );
    assert!(!history::track_history(&conn, &one, 20).unwrap().is_empty());
}

#[test]
fn hierarchical_tags_synonyms_and_quick_slots() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = small_library(tmp.path(), &["A - One.wav", "B - Two.wav", "C - Three.wav"]);
    let a = all(&conn, "artist:A")[0].id.clone();
    let b = all(&conn, "artist:B")[0].id.clone();

    let warm = tags::create(&conn, "Warm-up", "situation", None).unwrap();
    let early = tags::create(&conn, "Early warm-up", "situation", Some(&warm)).unwrap();
    let dark = tags::create(&conn, "Dark", "mood", None).unwrap();
    assert_eq!(
        tags::create(&conn, "  dark ", "mood", None).unwrap(),
        dark,
        "same name+category is reused"
    );
    tags::add_synonym(&conn, &dark, "Moody").unwrap();
    assert!(tags::add_synonym(&conn, &warm, "Moody").is_err());
    assert!(
        tags::set_parent(&conn, &warm, Some(&early)).is_err(),
        "cycle refused"
    );

    tags::assign(&conn, std::slice::from_ref(&a), &early, true).unwrap();
    tags::assign(&conn, std::slice::from_ref(&b), &dark, true).unwrap();
    assert_eq!(
        all(&conn, "tag:Warm-up").len(),
        1,
        "parent matches children"
    );
    assert_eq!(all(&conn, "tag:moody")[0].id, b, "synonym");
    assert_eq!(all(&conn, "tag:none").len(), 1);
    assert_eq!(all(&conn, "-tag:dark").len(), 2);

    // toggle on a mixed selection adds to all, then removes from all
    assert!(tags::toggle(&conn, &[a.clone(), b.clone()], &dark).unwrap());
    assert_eq!(all(&conn, "tag:dark").len(), 2);
    assert!(!tags::toggle(&conn, &[a.clone(), b.clone()], &dark).unwrap());
    assert_eq!(all(&conn, "tag:dark").len(), 0);
    history::undo(&conn).unwrap();
    assert_eq!(all(&conn, "tag:dark").len(), 2, "tag removal is undoable");

    tags::set_quick_slot(&conn, 1, Some(&dark)).unwrap();
    tags::set_quick_slot(&conn, 2, Some(&dark)).unwrap();
    assert_eq!(
        tags::quick_slot_tag(&conn, 1).unwrap(),
        None,
        "a tag occupies one slot"
    );
    assert_eq!(
        tags::quick_slot_tag(&conn, 2).unwrap().as_deref(),
        Some(dark.as_str())
    );

    // AI suggestions stay suggestions until reviewed
    tags::suggest(&conn, &a, &warm, "test-model", "0.1", None).unwrap();
    assert_eq!(
        all(&conn, "tag:warm-up").len(),
        1,
        "suggestion does not count as a tag"
    );
    tags::review_suggestion(&conn, &a, &warm, true).unwrap();
    let t = tags::for_track(&conn, &a).unwrap();
    assert!(t.iter().any(|x| x.tag_id == warm && x.origin == "user"));

    // deleting a parent re-parents children and removes assignments
    tags::delete(&conn, &warm).unwrap();
    let list = tags::list(&conn).unwrap();
    assert!(list.iter().any(|t| t.id == early && t.parent_id.is_none()));
}

#[test]
fn smart_fixes_preview_apply_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = small_library(tmp.path(), &["X - Y.wav"]);
    let id = all(&conn, "")[0].id.clone();
    library::edit_field(
        &conn,
        std::slice::from_ref(&id),
        "title",
        Some("01 - ROLLING GROOVE (Extended Mix) [www.promodj.com]"),
    )
    .unwrap();
    library::edit_field(
        &conn,
        std::slice::from_ref(&id),
        "artist",
        Some("Dj Bob ft. Ann"),
    )
    .unwrap();
    library::edit_field(
        &conn,
        std::slice::from_ref(&id),
        "genre",
        Some("tech-house"),
    )
    .unwrap();
    library::edit_field(&conn, std::slice::from_ref(&id), "version", None).unwrap();

    let rules: Vec<String> = fixes::RULES.iter().map(|r| r.id.to_string()).collect();
    let p = fixes::preview(&conn, std::slice::from_ref(&id), &rules, 100).unwrap();
    let get = |f: &str| {
        p.iter()
            .find(|x| x.field == f)
            .and_then(|x| x.new_value.clone())
    };
    assert_eq!(
        get("title").as_deref(),
        Some("Rolling Groove (Extended Mix)"),
        "{p:#?}"
    );
    assert_eq!(get("artist").as_deref(), Some("Dj Bob feat. Ann"));
    assert_eq!(get("genre").as_deref(), Some("Tech House"));
    assert_eq!(get("version").as_deref(), Some("Extended Mix"));
    // preview changed nothing
    assert_eq!(
        library::get_track(&conn, &id).unwrap().genre.as_deref(),
        Some("tech-house")
    );

    let accepted: Vec<fixes::AcceptedFix> = p
        .iter()
        .filter(|x| x.field != "artist")
        .map(|x| fixes::AcceptedFix {
            track_id: x.track_id.clone(),
            field: x.field.clone(),
            new_value: x.new_value.clone(),
        })
        .collect();
    assert_eq!(fixes::apply(&conn, &accepted).unwrap(), accepted.len());
    let t = library::get_track(&conn, &id).unwrap();
    assert_eq!(t.title.as_deref(), Some("Rolling Groove (Extended Mix)"));
    assert_eq!(
        t.artist.as_deref(),
        Some("Dj Bob ft. Ann"),
        "rejected proposal not applied"
    );
    let batch = history::undo(&conn).unwrap().unwrap();
    assert_eq!(batch.action, "smart-fix");
    assert_eq!(
        library::get_track(&conn, &id).unwrap().genre.as_deref(),
        Some("tech-house")
    );
    assert!(fixes::preview(&conn, &[id], &["nope".into()], 10).is_err());
}

#[test]
fn analysis_creates_phrase_cue_suggestions_and_user_cues_are_protected() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    // 128 BPM, 8 bars = 15 s; breakdown (-16 dB) from 60 s to 75 s.
    let spec = Spec {
        bpm: 128.0,
        tonic: 2,
        minor: true,
        seconds: 120.0,
        first_beat_s: 0.0,
    };
    let mut audio = synth(&spec, 44100);
    for (i, s) in audio.iter_mut().enumerate() {
        let t = (i / 2) as f32 / 44100.0;
        if (60.0..75.0).contains(&t) {
            *s *= 0.15;
        }
    }
    write_wav(&music.join("Z - Break.wav"), &audio, 44100);
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let id = all(&conn, "")[0].id.clone();
    // a user cue placed before analysis must survive it
    let user_cue = cues::add(
        &conn,
        &id,
        &cues::NewCue {
            kind: "hotcue",
            slot: Some(0),
            start_ms: 1234.0,
            end_ms: None,
            name: Some("Mine"),
            color: None,
        },
    )
    .unwrap();
    jobs::enqueue_analysis(&conn, std::slice::from_ref(&id), false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    let list = cues::list(&conn, &id).unwrap();
    let sugg: Vec<_> = list.iter().filter(|c| c.kind == "suggestion").collect();
    let find = |n: &str| {
        sugg.iter()
            .find(|c| c.name.as_deref() == Some(n))
            .map(|c| c.start_ms)
    };
    let beat = 60_000.0 / 128.0;
    let bd = find("Breakdown").expect("breakdown suggested");
    assert!((bd - 60_000.0).abs() < beat, "breakdown at {bd}");
    let drop = find("Drop").expect("drop suggested");
    assert!((drop - 75_000.0).abs() < beat, "drop at {drop}");
    assert!(
        find("Intro").is_some() && find("Outro").is_some(),
        "{sugg:#?}"
    );
    assert!(
        list.iter().any(|c| c.id == user_cue && c.locked),
        "user cue untouched"
    );

    // re-analysis replaces suggestions but keeps the user cue
    jobs::enqueue_analysis(&conn, std::slice::from_ref(&id), true).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    let list2 = cues::list(&conn, &id).unwrap();
    assert_eq!(
        list2.iter().filter(|c| c.kind == "suggestion").count(),
        sugg.len()
    );
    assert!(list2.iter().any(|c| c.id == user_cue));

    // accepting a suggestion makes a locked user cue; hot cue slot is unique
    let bd_id = list2
        .iter()
        .find(|c| c.name.as_deref() == Some("Breakdown"))
        .unwrap()
        .id
        .clone();
    let accepted = cues::accept(&conn, &bd_id, Some(0)).unwrap();
    let list3 = cues::list(&conn, &id).unwrap();
    let acc = list3.iter().find(|c| c.id == accepted).unwrap();
    assert_eq!(
        (acc.kind.as_str(), acc.slot, acc.locked, acc.source.as_str()),
        ("hotcue", Some(0), true, "user")
    );
    assert_eq!(
        list3.iter().find(|c| c.id == user_cue).unwrap().kind,
        "memory",
        "previous slot A became a memory cue"
    );
    // suggestions are not exported
    let xml = dc_core::rekordbox::build_xml(&conn, std::slice::from_ref(&id), "x").unwrap();
    assert_eq!(xml.matches("POSITION_MARK").count(), 2);
    assert!(
        cues::add(
            &conn,
            &id,
            &cues::NewCue {
                kind: "loop",
                slot: None,
                start_ms: 10.0,
                end_ms: None,
                name: None,
                color: None
            }
        )
        .is_err()
    );
    let _ = analysis::latest_result(&conn, &id).unwrap().unwrap();
}

#[test]
fn write_tags_back_to_files() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not available: skipping tag-writing test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let wav = make_track(
        tmp.path(),
        "src.wav",
        &Spec {
            bpm: 124.0,
            tonic: 9,
            minor: true,
            seconds: 6.0,
            first_beat_s: 0.0,
        },
    );
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let tags_in = [
        ("title", "Old Title"),
        ("artist", "Old Artist"),
        ("genre", "House"),
    ];
    let mut made = vec![];
    for (name, args) in [
        (
            "a.mp3",
            vec!["-c:a", "libmp3lame", "-b:a", "192k", "-id3v2_version", "3"],
        ),
        ("b.flac", vec!["-c:a", "flac"]),
        ("c.m4a", vec!["-c:a", "aac", "-b:a", "128k"]),
        ("d.aiff", vec!["-c:a", "pcm_s16be", "-write_id3v2", "1"]),
    ] {
        if ffmpeg_encode(&wav, &music.join(name), &args, &tags_in) {
            made.push(name);
        }
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let ids: Vec<String> = all(&conn, "").iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids.len(), made.len());
    library::edit_field(&conn, &ids, "title", Some("New Title (Dub Mix)")).unwrap();
    library::edit_field(&conn, &ids, "genre", Some("Deep Tech")).unwrap();
    library::edit_field(&conn, &ids, "bpm", Some("124.5")).unwrap();
    library::edit_field(&conn, &ids, "key", Some("Am")).unwrap();
    library::edit_field(&conn, &ids, "label", Some("Fold Records")).unwrap();
    library::edit_field(&conn, &ids, "year", Some("2021")).unwrap();

    let plan = tagwrite::plan(&conn, &ids).unwrap();
    assert_eq!(plan.len(), ids.len());
    assert!(
        plan[0]
            .changes
            .iter()
            .any(|c| c.field == "genre" && c.file_value.as_deref() == Some("House"))
    );

    // a file modified behind our back is refused
    let first = library::get_track(&conn, &ids[0]).unwrap().path.unwrap();
    let orig_bytes = std::fs::read(&first).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&first, &orig_bytes).unwrap(); // same content, new mtime
    let backups = tmp.path().join("tag-backups");
    let rep = tagwrite::write(&mut conn, &ids, Some(&backups)).unwrap();
    assert_eq!(rep.failed.len(), 1, "{rep:#?}");
    assert!(rep.failed[0].error.contains("changed on disk"));
    assert_eq!(rep.written, ids.len() - 1, "{rep:?}");
    let left = tagwrite::plan(&conn, &ids).unwrap();
    assert_eq!(
        left.len(),
        1,
        "only the refused file still differs: {left:#?}"
    );

    // after re-import the remaining file can be written too
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let rep = tagwrite::write(&mut conn, &ids, Some(&backups)).unwrap();
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert_eq!(rep.written, 1);
    assert!(
        tagwrite::plan(&conn, &ids).unwrap().is_empty(),
        "files now match the library"
    );

    for id in &ids {
        let p = library::get_track(&conn, id).unwrap().path.unwrap();
        let m = dc_core::metadata::read(Path::new(&p)).unwrap();
        assert_eq!(m.title.as_deref(), Some("New Title (Dub Mix)"), "{p}");
        assert_eq!(m.genre.as_deref(), Some("Deep Tech"), "{p}");
        assert_eq!(m.musical_key.as_deref(), Some("Am"), "{p}");
        assert_eq!(m.year, Some(2021), "{p}");
        assert!((m.bpm.unwrap() - 124.5).abs() <= 0.5, "{p}: {:?}", m.bpm);
        // audio still decodes
        dc_core::analysis::analyze_file(Path::new(&p), &|| false).unwrap();
    }
    // originals were copied before writing
    let copies: usize = std::fs::read_dir(&backups)
        .unwrap()
        .flatten()
        .map(|d| std::fs::read_dir(d.path()).unwrap().count())
        .sum();
    assert_eq!(copies, ids.len());
    // incremental re-import sees no changes (DB was refreshed after writing)
    let rep = library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    assert_eq!(rep.unchanged, ids.len());
}

#[test]
fn watch_folder_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let key = library::add_watch_folder(&conn, tmp.path()).unwrap();
    library::add_watch_folder(&conn, tmp.path()).unwrap();
    assert_eq!(library::watch_folders(&conn).unwrap().len(), 1);
    assert!(library::add_watch_folder(&conn, &tmp.path().join("nope")).is_err());
    library::mark_watch_scanned(&conn, &key).unwrap();
    assert!(
        library::watch_folders(&conn).unwrap()[0]
            .last_scan
            .is_some()
    );
    library::remove_watch_folder(&conn, &key).unwrap();
    assert!(library::watch_folders(&conn).unwrap().is_empty());
}
