//! Cue templates: anchors, bar offsets on the grid, slot policy, undo.

mod common;

use common::*;
use dc_core::cues::{self, Anchor, Template, TemplateSlot};
use dc_core::{db, history, jobs, library};

fn slot(anchor: Anchor, offset_bars: f64, kind: &str, hot: Option<i64>) -> TemplateSlot {
    TemplateSlot {
        anchor,
        occurrence: 1,
        offset_bars,
        kind: kind.into(),
        hot,
        loop_bars: (kind == "loop").then_some(4.0),
        name: Some(format!("{anchor:?}")),
        color: None,
    }
}

#[test]
fn templates_place_cues_on_anchors_and_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    // 128 BPM (one bar = 1875 ms), breakdown from 60 s to 75 s.
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
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id: String = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .unwrap();
    jobs::enqueue_analysis(&conn, std::slice::from_ref(&id), false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    // An existing user hot cue in slot B.
    cues::add(
        &conn,
        &id,
        &cues::NewCue {
            kind: "hotcue",
            slot: Some(1),
            start_ms: 3750.0,
            end_ms: None,
            name: Some("Mine"),
            color: None,
        },
    )
    .unwrap();

    let t = cues::save_template(
        &conn,
        Template {
            id: String::new(),
            name: "Club".into(),
            slots: vec![
                slot(Anchor::FirstBeat, 0.0, "hotcue", Some(0)),
                slot(Anchor::Breakdown, -8.0, "hotcue", Some(1)),
                slot(Anchor::Drop, 0.0, "hotcue", Some(2)),
                slot(Anchor::Breakdown, 0.0, "loop", None),
                slot(Anchor::End, -16.0, "memory", None),
            ],
            replace_hot: false,
        },
    )
    .unwrap();
    assert_eq!(cues::list_templates(&conn).unwrap().len(), 1);
    let before = cues::list(&conn, &id).unwrap().len();
    let rep = cues::apply_template(&conn, &t, std::slice::from_ref(&id)).unwrap();
    // Slot B was in use and replace_hot is off.
    assert_eq!(rep.created, 4, "{rep:?}");
    assert!(
        rep.skipped.iter().any(|(_, r)| r.contains("hot cue B")),
        "{rep:?}"
    );
    let list = cues::list(&conn, &id).unwrap();
    let get = |kind: &str, slot: Option<i64>| {
        list.iter()
            .find(|c| c.kind == kind && (slot.is_none() || c.slot == slot) && c.source == "user")
            .map(|c| (c.start_ms, c.end_ms))
    };
    let bar = 60_000.0 / 128.0 * 4.0;
    let (a, _) = get("hotcue", Some(0)).unwrap();
    assert!(a < 100.0, "first beat {a}");
    // Breakdown at 60 s (32 bars); drop at 75 s; both on the grid.
    let (d, _) = get("hotcue", Some(2)).unwrap();
    assert!((d - 75_000.0).abs() <= bar / 2.0 + 50.0, "drop at {d}");
    let (ls, le) = get("loop", None).unwrap();
    assert!((ls - 60_000.0).abs() <= bar / 2.0 + 50.0, "loop at {ls}");
    assert!((le.unwrap() - ls - 4.0 * bar).abs() < 1.0);
    let grid_off = |x: f64| ((x - a) / (bar / 4.0)).fract().abs();
    assert!(grid_off(d) < 0.01 || grid_off(d) > 0.99);
    // Mine is untouched.
    assert!(
        list.iter()
            .any(|c| c.name.as_deref() == Some("Mine") && c.slot == Some(1))
    );

    // Undo removes exactly what the template created.
    history::undo(&conn).unwrap();
    assert_eq!(cues::list(&conn, &id).unwrap().len(), before);

    // With replace_hot, B is taken and Mine becomes a memory cue; undo restores it.
    let t2 = Template {
        replace_hot: true,
        ..t
    };
    let rep = cues::apply_template(&conn, &t2, std::slice::from_ref(&id)).unwrap();
    assert_eq!(rep.created, 5, "{rep:?}");
    let list = cues::list(&conn, &id).unwrap();
    let mine = list
        .iter()
        .find(|c| c.name.as_deref() == Some("Mine"))
        .unwrap();
    assert_eq!((mine.kind.as_str(), mine.slot), ("memory", None));
    history::undo(&conn).unwrap();
    let list = cues::list(&conn, &id).unwrap();
    let mine = list
        .iter()
        .find(|c| c.name.as_deref() == Some("Mine"))
        .unwrap();
    assert_eq!((mine.kind.as_str(), mine.slot), ("hotcue", Some(1)));
    assert_eq!(list.len(), before);

    // Validation.
    let bad = Template {
        id: String::new(),
        name: "x".into(),
        slots: vec![slot(Anchor::FirstBeat, 0.0, "hotcue", None)],
        replace_hot: false,
    };
    assert!(cues::save_template(&conn, bad).is_err());
}
