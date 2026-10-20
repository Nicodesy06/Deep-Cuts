//! Analyser checks on synthetic signals with exact ground truth. These
//! verify the DSP implementation; they are NOT a measure of accuracy on real
//! music (see docs/AI_VALIDATION.md for the real-corpus protocol).

mod common;

use common::*;
use dc_core::analysis::analyze_file;

#[test]
fn tempo_and_key_sweep() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: &[(f32, u8, bool, &str)] = &[
        (90.0, 2, true, "7A"),    // D minor
        (100.0, 5, false, "7B"),  // F major
        (118.5, 1, true, "12A"),  // C# minor
        (122.0, 7, false, "9B"),  // G major
        (125.0, 4, true, "9A"),   // E minor
        (130.0, 10, false, "6B"), // Bb major
        (140.0, 11, true, "10A"), // B minor
        (174.0, 6, true, "11A"),  // F# minor
    ];
    let mut failures = vec![];
    for (i, &(bpm, tonic, minor, camelot)) in cases.iter().enumerate() {
        let p = make_track(
            tmp.path(),
            &format!("t{i}.wav"),
            &Spec {
                bpm,
                tonic,
                minor,
                seconds: 30.0,
                first_beat_s: 0.1,
            },
        );
        let a = analyze_file(&p, &|| false).unwrap().result;
        let got_bpm = a.tempo.as_ref().map(|t| t.bpm).unwrap_or(0.0);
        let got_key = a
            .key
            .as_ref()
            .map(|k| k.camelot.clone())
            .unwrap_or_default();
        println!(
            "{bpm:>6} {camelot:>4} -> {got_bpm:>7.2} {got_key:>4} periodicity {:.3} key r {:.3}",
            a.tempo.as_ref().map(|t| t.periodicity).unwrap_or(0.0),
            a.key.as_ref().map(|k| k.correlation).unwrap_or(0.0)
        );
        if (got_bpm - bpm as f64).abs() > 0.1 || got_key != camelot {
            failures.push(format!("{bpm} {camelot}: got {got_bpm} {got_key}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn silence_and_short_files() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("silence.wav");
    write_wav(&p, &vec![0.0; 44100 * 2 * 12], 44100);
    let a = analyze_file(&p, &|| false).unwrap().result;
    assert!(a.tempo.is_none(), "no tempo in silence: {:?}", a.tempo);
    assert!(a.key.is_none());
    assert_eq!(a.loudness.integrated_lufs, None);
    let short = tmp.path().join("short.wav");
    write_wav(
        &short,
        &synth(
            &Spec {
                bpm: 120.0,
                tonic: 0,
                minor: false,
                seconds: 3.0,
                first_beat_s: 0.0,
            },
            44100,
        ),
        44100,
    );
    let a = analyze_file(&short, &|| false).unwrap().result;
    assert!(a.tempo.is_none(), "too short for a tempo estimate");
    assert_eq!(a.duration_ms, 3000);
}

#[test]
fn cancellation() {
    let tmp = tempfile::tempdir().unwrap();
    let p = make_track(
        tmp.path(),
        "c.wav",
        &Spec {
            bpm: 120.0,
            tonic: 0,
            minor: false,
            seconds: 10.0,
            first_beat_s: 0.0,
        },
    );
    assert!(matches!(
        analyze_file(&p, &|| true),
        Err(dc_core::Error::Cancelled)
    ));
}
