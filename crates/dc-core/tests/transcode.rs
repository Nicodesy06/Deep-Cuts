//! Bandwidth measurement and transcoding detection on real encoder output.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, jobs, library};

/// Stereo-interleaved music-like signal with content up to 22 kHz:
/// a synthetic groove plus decaying broadband "hats".
fn broadband(seconds: f32, sr: u32) -> Vec<f32> {
    let mut x = synth(
        &Spec {
            bpm: 126.0,
            tonic: 7,
            minor: true,
            seconds,
            first_beat_s: 0.0,
        },
        sr,
    );
    let mut state = 99u64;
    let eighth = (60.0 / 126.0 / 2.0 * sr as f64) as usize;
    for (i, pair) in x.chunks_exact_mut(2).enumerate() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = ((state >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 0.6;
        let env = (-((i % eighth) as f32) / (0.04 * sr as f32)).exp();
        let v = n * (0.08 + env);
        pair[0] += v;
        pair[1] += v;
    }
    x
}

#[test]
fn fake_lossless_and_fake_320_are_flagged() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not found: skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&music).unwrap();
    let wav = work.join("src.wav");
    write_wav(&wav, &broadband(30.0, 44100), 44100);
    let mp3_128 = work.join("low.mp3");
    assert!(ffmpeg_encode(
        &wav,
        &mp3_128,
        &["-c:a", "libmp3lame", "-b:a", "128k"],
        &[]
    ));
    let ok = |src: &std::path::Path, name: &str, args: &[&str]| {
        assert!(ffmpeg_encode(
            src,
            &music.join(name),
            args,
            &[("artist", name), ("title", "t")]
        ));
    };
    ok(&wav, "genuine.flac", &["-c:a", "flac"]);
    ok(&mp3_128, "fake.flac", &["-c:a", "flac"]);
    ok(
        &wav,
        "genuine320.mp3",
        &["-c:a", "libmp3lame", "-b:a", "320k"],
    );
    ok(
        &mp3_128,
        "fake320.mp3",
        &["-c:a", "libmp3lame", "-b:a", "320k"],
    );
    ok(&mp3_128, "honest128.mp3", &["-c:a", "copy"]);

    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let all = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let ids: Vec<String> = all.iter().map(|t| t.id.clone()).collect();
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}

    let suspects = library::transcode_suspects(&conn, 100).unwrap();
    let mut flagged: Vec<String> = suspects
        .iter()
        .map(|s| s.track.artist.clone().unwrap())
        .collect();
    flagged.sort();
    assert_eq!(flagged, vec!["fake.flac", "fake320.mp3"], "{suspects:#?}");
    for s in &suspects {
        let c = s.cutoff_hz.unwrap();
        assert!(
            (15_000.0..18_500.0).contains(&c),
            "128k LAME low-pass, got {c}"
        );
    }
    let fake = suspects
        .iter()
        .find(|s| s.track.artist.as_deref() == Some("fake.flac"))
        .unwrap();
    assert_eq!(fake.reason, "lossless-band-limited");
    assert_eq!(fake.codec.as_deref(), Some("flac"));
    // the same list is reachable from the query language
    let q = query_tracks(
        &conn,
        &TrackQuery {
            query: "transcode:yes".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(q.tracks.len(), 2);
}
