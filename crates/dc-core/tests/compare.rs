//! Analysis vs imported BPM/key on the user's library; BPM range folding.

mod common;

use common::*;
use dc_core::{analysis, compare, db, library};

#[test]
fn compare_with_imported_values_and_bpm_range() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    for (name, bpm) in [("a.wav", 124.0), ("b.wav", 128.0), ("c.wav", 174.0)] {
        make_track(
            &music,
            name,
            &Spec {
                bpm,
                tonic: 0,
                minor: false,
                seconds: 24.0,
                first_beat_s: 0.0,
            },
        );
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id = |f: &str| -> String {
        conn.query_row(
            "SELECT track_id FROM files WHERE path LIKE '%' || ?1",
            [f],
            |r| r.get(0),
        )
        .unwrap()
    };
    for f in ["a.wav", "b.wav", "c.wav"] {
        let p = library::track_path(&conn, &id(f)).unwrap();
        let a = analysis::analyze_file(&p, &|| false).unwrap();
        analysis::store(&conn, &id(f), &a, 0).unwrap();
    }
    let ours = |f: &str| -> f64 { library::get_track(&conn, &id(f)).unwrap().bpm.unwrap() };
    assert!((ours("a.wav") - 124.0).abs() < 0.05);
    // "Rekordbox" says: a exact, b half, c unrelated; keys: same / relative.
    let src = "import:rekordbox";
    library::set_field(&conn, &id("a.wav"), "bpm", src, Some("124.00")).unwrap();
    library::set_field(&conn, &id("b.wav"), "bpm", src, Some("64")).unwrap();
    library::set_field(&conn, &id("c.wav"), "bpm", src, Some("100")).unwrap();
    library::set_field(&conn, &id("a.wav"), "key", src, Some("8B")).unwrap();
    library::set_field(&conn, &id("b.wav"), "key", src, Some("Am")).unwrap();

    let s = compare::sources(&conn).unwrap();
    assert!(
        s.iter()
            .any(|c| c.source == src && c.bpm == 3 && c.key == 2),
        "{s:?}"
    );
    let rep = compare::compare(&conn, src, 50).unwrap();
    assert_eq!(
        (
            rep.bpm_compared,
            rep.bpm_exact,
            rep.bpm_octave,
            rep.bpm_other
        ),
        (3, 1, 1, 1),
        "{rep:?}"
    );
    assert_eq!(
        (rep.key_compared, rep.key_same, rep.key_relative),
        (2, 1, 1)
    );
    // Largest disagreements first.
    assert_eq!(rep.differences[0].kind, "bpm-other");
    assert_eq!(rep.differences[0].track.id, id("c.wav"));

    // A user range folds the analysed tempo by octaves only.
    let p = library::track_path(&conn, &id("b.wav")).unwrap();
    let opts = analysis::AnalyzeOptions {
        bpm_range: Some((60.0, 120.0)),
        ..Default::default()
    };
    let a = analysis::analyze_file_with(&p, &|| false, &opts).unwrap();
    let t = a.result.tempo.unwrap();
    assert!((t.bpm - 64.0).abs() < 0.05, "{}", t.bpm);
    assert_eq!(t.range, Some((60.0, 120.0)));
    // The setting is read by the analysis queue.
    library::set_setting(&conn, "bpmRange", "60-120").unwrap();
    assert_eq!(
        analysis::AnalyzeOptions::from_settings(&conn).bpm_range,
        Some((60.0, 120.0))
    );
}
