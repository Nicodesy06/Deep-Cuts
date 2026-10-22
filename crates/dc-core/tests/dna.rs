//! Energy rank, label suggestions learned from the user's labels (with
//! leave-one-out accuracy), auto groups.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, dna, jobs, library, tags};

#[test]
fn energy_suggestions_and_groups_follow_the_music() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    // Two styles: fast, loud, bright (with hats) vs slow, quiet, dark.
    for i in 0..8 {
        let spec = Spec {
            bpm: 136.0 + i as f32,
            tonic: (i % 12) as u8,
            minor: true,
            seconds: 20.0,
            first_beat_s: 0.0,
        };
        write_wav(
            &music.join(format!("fast{i}.wav")),
            &broadband(&spec, 22050, i as u64 + 1),
            22050,
        );
        let spec = Spec {
            bpm: 92.0 + i as f32,
            tonic: (i % 12) as u8,
            minor: false,
            seconds: 20.0,
            first_beat_s: 0.0,
        };
        let mut quiet = synth(&spec, 22050);
        quiet.iter_mut().for_each(|s| *s *= 0.25);
        write_wav(&music.join(format!("slow{i}.wav")), &quiet, 22050);
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let rows = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    let id = |n: &str| {
        rows.iter()
            .find(|r| r.path.as_deref().unwrap().ends_with(n))
            .unwrap()
            .id
            .clone()
    };

    // Energy 1–10: every fast track ranks above every slow one.
    assert_eq!(dna::update_energy(&conn).unwrap(), 16);
    let energy = |n: &str| library::get_track(&conn, &id(n)).unwrap().energy.unwrap();
    let min_fast = (0..8)
        .map(|i| energy(&format!("fast{i}.wav")))
        .min()
        .unwrap();
    let max_slow = (0..8)
        .map(|i| energy(&format!("slow{i}.wav")))
        .max()
        .unwrap();
    assert!(min_fast > max_slow, "fast ≥ {min_fast}, slow ≤ {max_slow}");

    // Mixed In Key energies in comments: read as references, then the energy
    // is calibrated on them and checked by cross-validation.
    let (_, before) = dna::compute_energy(&conn).unwrap();
    assert_eq!(before.method, "relative");
    assert_eq!(before.references, 0);
    for i in 0..6 {
        library::edit_field(
            &conn,
            &[id(&format!("fast{i}.wav"))],
            "comment",
            Some("8A - Energy 9"),
        )
        .unwrap();
        library::edit_field(
            &conn,
            &[id(&format!("slow{i}.wav"))],
            "comment",
            Some("Energy 2"),
        )
        .unwrap();
    }
    assert_eq!(dna::energy_references(&conn).unwrap().len(), 12);
    let (levels, rep) = dna::compute_energy_with(&conn, 10).unwrap();
    assert_eq!(rep.method, "calibrated");
    assert_eq!(rep.references, 12);
    let cal = rep.calibrated.unwrap();
    assert!(cal.within_one >= 0.9, "{cal:?}");
    assert!(levels[&id("fast7.wav")] >= 7, "{levels:?}");
    assert!(levels[&id("slow7.wav")] <= 4, "{levels:?}");
    // Below the threshold the energy stays relative to the library.
    assert_eq!(dna::compute_energy(&conn).unwrap().1.method, "relative");
    assert_eq!(
        query_tracks(
            &conn,
            &TrackQuery {
                query: "energy:>=6".into(),
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        8
    );

    // Label 6 of each style; the remaining 2+2 get suggestions.
    for i in 0..6 {
        library::edit_field(
            &conn,
            &[id(&format!("fast{i}.wav"))],
            "genre",
            Some("Techno"),
        )
        .unwrap();
        library::edit_field(
            &conn,
            &[id(&format!("slow{i}.wav"))],
            "genre",
            Some("Downtempo"),
        )
        .unwrap();
    }
    library::edit_field(&conn, &[id("fast0.wav")], "genre", Some("techno")).unwrap(); // spelling variant
    let rep = dna::genre_suggestions(&conn, 0.5).unwrap();
    let e = &rep.eval["all"];
    assert_eq!(
        (e.labelled, e.suggested, e.correct),
        (12, 12, 12),
        "leave-one-out on the user's labels"
    );
    let mut got: Vec<(String, String)> = rep
        .suggestions
        .iter()
        .map(|s| {
            (
                library::get_track(&conn, &s.track_id)
                    .unwrap()
                    .path
                    .unwrap(),
                s.label.clone(),
            )
        })
        .map(|(p, l)| {
            (
                std::path::Path::new(&p)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                l,
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            ("fast6.wav".into(), "Techno".into()),
            ("fast7.wav".into(), "Techno".into()),
            ("slow6.wav".into(), "Downtempo".into()),
            ("slow7.wav".into(), "Downtempo".into())
        ]
    );

    // Tags: "Peak" on five fast tracks → suggested for the other fast ones only.
    let peak = tags::ensure(&conn, "Peak", "mood").unwrap();
    let fast_ids: Vec<String> = (0..5).map(|i| id(&format!("fast{i}.wav"))).collect();
    tags::assign(&conn, &fast_ids, &peak, true).unwrap();
    let rep = dna::tag_suggestions(&conn, 0.5, 5).unwrap();
    let suggested: Vec<String> = rep.suggestions.iter().map(|s| s.track_id.clone()).collect();
    assert!(!suggested.is_empty());
    assert!(
        suggested
            .iter()
            .all(|t| (5..8).any(|i| *t == id(&format!("fast{i}.wav")))),
        "only fast tracks"
    );
    assert_eq!(
        dna::store_tag_suggestions(&conn, &rep).unwrap(),
        suggested.len()
    );
    // A rejected suggestion is not offered again.
    tags::review_suggestion(&conn, &suggested[0], &peak, false).unwrap();
    let again = dna::tag_suggestions(&conn, 0.5, 5).unwrap();
    assert!(again.suggestions.iter().all(|s| s.track_id != suggested[0]));
    // Undoing a review makes it pending again; undoing an accept removes the tag.
    let pending = |c: &rusqlite::Connection| -> i64 {
        c.query_row(
            "SELECT count(*) FROM track_tags WHERE origin = 'ai' AND status = 'suggested'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let before = pending(&conn);
    tags::unreview_suggestion(&conn, &suggested[0], &peak).unwrap();
    assert_eq!(pending(&conn), before + 1);
    tags::review_suggestion(&conn, &suggested[0], &peak, true).unwrap();
    let has_peak = |c: &rusqlite::Connection| -> i64 {
        c.query_row(
            "SELECT count(*) FROM track_tags WHERE track_id = ?1 AND tag_id = ?2 AND origin = 'user'",
            [&suggested[0], &peak],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(has_peak(&conn), 1);
    tags::unreview_suggestion(&conn, &suggested[0], &peak).unwrap();
    assert_eq!(has_peak(&conn), 0);
    assert_eq!(pending(&conn), before + 1);
    tags::review_suggestion(&conn, &suggested[0], &peak, false).unwrap();

    // Auto groups: two groups that split the styles.
    let groups = dna::auto_groups(&conn, 2, "").unwrap();
    assert_eq!(groups.len(), 2);
    for g in &groups {
        let fast = g
            .tracks
            .iter()
            .filter(|t| (0..8).any(|i| **t == id(&format!("fast{i}.wav"))))
            .count();
        assert!(
            fast == 0 || fast == g.tracks.len(),
            "pure groups: {fast}/{}",
            g.tracks.len()
        );
        assert!(!g.traits.is_empty());
    }
}
