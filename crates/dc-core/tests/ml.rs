//! The bundled audio tagging model loads and gives sensible outputs.

mod common;

use common::*;
use dc_core::ml;

#[test]
fn model_tags_music_and_silence() {
    let tagger = ml::Tagger::load_default().expect("model file present in models/");
    eprintln!("device: {}", tagger.device);
    assert_eq!(ml::LABELS.len(), 527);
    // Every kept label exists in the model's label list.
    for (_, labels) in ml::GROUPS {
        for l in *labels {
            assert!(ml::LABELS.contains(l), "unknown AudioSet label {l}");
        }
    }
    // A synthetic groove (kick, tonal chord, hats) is music; silence is not.
    let spec = Spec {
        bpm: 126.0,
        tonic: 0,
        minor: true,
        seconds: 20.0,
        first_beat_s: 0.0,
    };
    let stereo = synth(&spec, 44100);
    let mono: Vec<f32> = stereo.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect();
    let p = tagger.tag_signal(&mono, 44100).unwrap();
    assert_eq!(p.len(), 527);
    let music = ml::LABELS.iter().position(|l| *l == "Music").unwrap();
    let silence = ml::LABELS.iter().position(|l| *l == "Silence").unwrap();
    eprintln!("music {} silence {}", p[music], p[silence]);
    let top: Vec<_> = ml::kept(&p).into_iter().take(5).collect();
    eprintln!("top kept: {top:?}");
    assert!(p[music] > 0.3, "music {}", p[music]);
    let quiet = vec![0f32; 44100 * 12];
    let q = tagger.tag_signal(&quiet, 44100).unwrap();
    assert!(
        q[silence] > q[music],
        "silence {} music {}",
        q[silence],
        q[music]
    );
}

#[test]
fn analysis_stores_model_tags_search_and_suggestions() {
    use dc_core::query::{TrackQuery, query_tracks};
    use dc_core::{db, dna, jobs, library, tags};
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    for (n, bpm) in [("a.wav", 126.0), ("b.wav", 128.0)] {
        make_track(
            &music,
            n,
            &Spec {
                bpm,
                tonic: 0,
                minor: true,
                seconds: 20.0,
                first_beat_s: 0.0,
            },
        );
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let ids: Vec<String> = {
        let mut st = conn.prepare("SELECT id FROM tracks ORDER BY id").unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    let tags = ml::for_track(&conn, &ids[0]).unwrap();
    assert!(!tags.is_empty());
    assert!(tags.iter().any(|t| t.group == "genre" && t.rank == 1));
    let q = |s: &str| {
        query_tracks(
            &conn,
            &TrackQuery {
                query: s.into(),
                ..Default::default()
            },
        )
        .unwrap()
        .total
    };
    assert_eq!(q("ai:\"electronic music\""), 2, "{tags:?}");
    assert_eq!(q("ai:opera"), 0);
    // Suggestions go to the review queue, not straight to tags.
    let n = ml::suggest_tags(&conn, None).unwrap();
    assert!(n > 0);
    let tt = tags::for_track(&conn, &ids[0]).unwrap();
    assert!(
        tt.iter()
            .all(|t| t.status == "suggested" && t.origin == "ai"),
        "{tt:?}"
    );
    let c = dna::character(&conn, &ids[0]).unwrap();
    assert!(c.bass.is_some() && c.drive.is_some() && c.steadiness.is_some());
    // Turned off: the analysis does not run the model.
    library::set_setting(&conn, "aiModel", "off").unwrap();
    assert!(
        dc_core::analysis::AnalyzeOptions::from_settings(&conn)
            .tagger
            .is_none()
    );
}
