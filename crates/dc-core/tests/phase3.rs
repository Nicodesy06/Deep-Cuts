//! Similarity, discovery map and set preparation on analysed audio.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, features, history, jobs, library, map, sets, similarity};

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

/// Five analysed tracks: three 124–126 BPM in A minor-ish keys, two far away.
fn analysed_library(dir: &std::path::Path) -> rusqlite::Connection {
    let music = dir.join("m");
    std::fs::create_dir_all(&music).unwrap();
    let specs = [
        ("A - Base.wav", 124.0, 9, true, 1.0),
        ("B - Twin.wav", 125.0, 4, true, 1.0), // E minor: adjacent to A minor
        ("C - Close.wav", 126.0, 9, true, 0.8),
        ("D - Fast.wav", 140.0, 1, false, 0.3), // Db major, quiet
        ("E - Slow.wav", 100.0, 6, false, 0.5),
    ];
    for (name, bpm, tonic, minor, gain) in specs {
        let mut audio = synth(
            &Spec {
                bpm,
                tonic,
                minor,
                seconds: 24.0,
                first_beat_s: 0.0,
            },
            44100,
        );
        audio.iter_mut().for_each(|s| *s *= gain);
        write_wav(&music.join(name), &audio, 44100);
    }
    let mut conn = db::open(&dir.join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let ids: Vec<String> = all(&conn, "").iter().map(|t| t.id.clone()).collect();
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    conn
}

fn id_of(conn: &rusqlite::Connection, artist: &str) -> String {
    all(conn, &format!("artist:\"{artist}\""))[0].id.clone()
}

#[test]
fn similar_tracks_are_ranked_and_explained() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = analysed_library(tmp.path());
    let base = id_of(&conn, "A");
    let res = similarity::similar(&conn, &base, &similarity::Weights::default(), "", 10).unwrap();
    assert_eq!(res.len(), 4);
    let names: Vec<&str> = res
        .iter()
        .map(|m| m.track.artist.as_deref().unwrap())
        .collect();
    assert!(
        names[..2].contains(&"B") && names[..2].contains(&"C"),
        "{names:?}"
    );
    assert!(res[0].score >= res[3].score);
    let b = res
        .iter()
        .find(|m| m.track.artist.as_deref() == Some("B"))
        .unwrap();
    assert_eq!(b.facts.harmonic, similarity::Harmonic::Adjacent);
    assert!(
        b.groups
            .iter()
            .any(|g| g.group == "tempo" && g.similarity.unwrap() > 0.8)
    );
    // scope restricts candidates
    let scoped = similarity::similar(
        &conn,
        &base,
        &similarity::Weights::default(),
        "bpm:>130",
        10,
    )
    .unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].track.artist.as_deref(), Some("D"));
    // a user key correction is respected
    library::edit_field(&conn, &[id_of(&conn, "D")], "key", Some("8A")).unwrap();
    let again = similarity::similar(
        &conn,
        &base,
        &similarity::Weights::default(),
        "artist:D",
        10,
    )
    .unwrap();
    assert_eq!(again[0].facts.harmonic, similarity::Harmonic::Same);
}

#[test]
fn discovery_map_projects_all_analysed_tracks() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = analysed_library(tmp.path());
    let m = map::build(&conn, "").unwrap();
    assert_eq!(m.points.len(), 5);
    assert_eq!(m.axes.len(), 2);
    assert!(m.axes[0].explained >= m.axes[1].explained);
    assert!(m.points.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    let scoped = map::build(&conn, "bpm:120-130").unwrap();
    assert_eq!(scoped.points.len(), 3);
    // features are backfilled for tracks analysed before schema v3
    conn.execute("DELETE FROM track_features", []).unwrap();
    assert_eq!(features::backfill(&conn).unwrap(), 5);
}

#[test]
fn set_builder_chapters_locks_sort_suggest_and_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = analysed_library(tmp.path());
    let (a, b, c, d, e) = (
        id_of(&conn, "A"),
        id_of(&conn, "B"),
        id_of(&conn, "C"),
        id_of(&conn, "D"),
        id_of(&conn, "E"),
    );
    let set = library::create_playlist(&conn, "Friday set", None, "set", None).unwrap();
    library::reorder_playlist(
        &conn,
        &set,
        &[d.clone(), a.clone(), e.clone(), c.clone()],
        "playlist-add",
    )
    .unwrap();
    sets::set_chapter(&conn, &set, 0, "opening", None).unwrap();
    sets::set_chapter(&conn, &set, 2, "peak", Some("Main room")).unwrap();
    assert!(sets::set_chapter(&conn, &set, 1, "nonsense", None).is_err());
    sets::set_lock(&conn, &set, &a, true).unwrap();

    let rep = sets::report(&conn, &set, "smooth").unwrap();
    assert_eq!(rep.tracks.len(), 4);
    assert_eq!(rep.chapters.len(), 2);
    assert_eq!(rep.tracks[3].chapter.as_deref(), Some("peak"));
    assert!(rep.tracks[1].locked);
    assert!(rep.tracks[0].transition.is_none() && rep.tracks[1].transition.is_some());
    assert_eq!(rep.unanalyzed, 0);
    assert!(rep.duration_ms > 90_000);

    let sorted = sets::magic_sort(&conn, &set, "smooth").unwrap();
    assert_eq!(sorted.order[1], a, "pinned track stays at index 1");
    assert!(sorted.cost_after <= sorted.cost_before);
    sets::apply_order(&conn, &set, &sorted.order).unwrap();
    assert_eq!(
        library::playlist_track_ids(&conn, &set).unwrap(),
        sorted.order
    );
    assert!(
        sets::apply_order(&conn, &set, std::slice::from_ref(&a)).is_err(),
        "must be a permutation"
    );

    // undo restores the previous order
    let undone = history::undo(&conn).unwrap().unwrap();
    assert_eq!(undone.action, "set-sort");
    assert_eq!(
        library::playlist_track_ids(&conn, &set).unwrap(),
        vec![d.clone(), a.clone(), e.clone(), c.clone()]
    );

    // next-track suggestions exclude set members and passes, best match first
    library::set_decision(&conn, std::slice::from_ref(&e), "global", Some("pass")).unwrap();
    let next = sets::suggest_next(&conn, &set, Some(1), "", "smooth", 5).unwrap();
    let ids: Vec<&str> = next.iter().map(|x| x.track.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![b.as_str()],
        "only B is outside the set and not passed"
    );
    assert!(next[0].fit > 0.4);
}

#[test]
fn graph_path_from_seed_prefers_smooth_transitions() {
    let tmp = tempfile::tempdir().unwrap();
    let conn = analysed_library(tmp.path());
    let a = id_of(&conn, "A");
    let path = sets::build_path(&conn, &a, 3, "", "smooth").unwrap();
    assert_eq!(path.len(), 3);
    assert_eq!(path[0], a);
    let names: Vec<String> = path
        .iter()
        .map(|i| library::get_track(&conn, i).unwrap().artist.unwrap())
        .collect();
    // From A (124 BPM, A minor) the close tracks B and C come before the
    // far-away D (140) and E (100).
    assert!(names[1..].iter().all(|n| n == "B" || n == "C"), "{names:?}");
    // Scope restricts the candidates; the pool may run out.
    let short = sets::build_path(&conn, &a, 5, "bpm:>130", "smooth").unwrap();
    assert_eq!(short.len(), 2);
    assert!(sets::build_path(&conn, "nope", 3, "", "smooth").is_err());
}
