//! Acoustic duplicates: the same recording across formats/bitrates is
//! grouped; different recordings are not.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, jobs, library};

fn spec(bpm: f32, tonic: u8) -> Spec {
    Spec {
        bpm,
        tonic,
        minor: true,
        seconds: 140.0,
        first_beat_s: 0.0,
    }
}

#[test]
fn same_recording_across_formats_is_grouped() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not found: skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&music).unwrap();
    let a = work.join("a.wav");
    write_wav(&a, &broadband(&spec(124.0, 9), 44100, 1), 44100);
    let other_key = work.join("b.wav");
    write_wav(&other_key, &broadband(&spec(124.0, 2), 44100, 7), 44100);
    let other_tempo = work.join("c.wav");
    write_wav(&other_tempo, &broadband(&spec(128.0, 9), 44100, 3), 44100);
    let enc = |src: &std::path::Path, name: &str, args: &[&str]| {
        assert!(ffmpeg_encode(
            src,
            &music.join(name),
            args,
            &[("artist", name), ("title", "t")]
        ));
    };
    enc(&a, "A.flac", &["-c:a", "flac"]);
    enc(&a, "A-320.mp3", &["-c:a", "libmp3lame", "-b:a", "320k"]);
    enc(&a, "A-128.mp3", &["-c:a", "libmp3lame", "-b:a", "128k"]);
    enc(&a, "A-aac.m4a", &["-c:a", "aac", "-b:a", "192k"]);
    enc(
        &a,
        "A-quieter.flac",
        &["-af", "volume=-6dB", "-c:a", "flac"],
    );
    enc(
        &a,
        "A-trimmed.flac",
        &["-af", "atrim=start=0.4", "-c:a", "flac"],
    );
    enc(&other_key, "B.flac", &["-c:a", "flac"]);
    enc(&other_tempo, "C.flac", &["-c:a", "flac"]);

    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, &[music], None, &mut |_| {}).unwrap();
    let all = query_tracks(&conn, &TrackQuery::default()).unwrap().tracks;
    let ids: Vec<String> = all.iter().map(|t| t.id.clone()).collect();
    jobs::enqueue_analysis(&conn, &ids, false).unwrap();
    while jobs::run_one(&conn, &|| false).unwrap().is_some() {}
    let name = |id: &str| {
        all.iter()
            .find(|t| t.id == id)
            .unwrap()
            .artist
            .clone()
            .unwrap()
    };

    // Every compared pair with its bit-error rate (for the record).
    let mut pairs: Vec<(String, String, f64)> = library::acoustic_pairs(&conn, 1.0)
        .unwrap()
        .into_iter()
        .map(|(x, y, d)| {
            let (x, y) = (name(&x), name(&y));
            if x < y { (x, y, d) } else { (y, x, d) }
        })
        .collect();
    pairs.sort_by(|p, q| p.2.total_cmp(&q.2));
    for (x, y, d) in &pairs {
        eprintln!("{d:.3}  {x} ~ {y}");
    }
    let same = |n: &str| n.starts_with("A");
    let worst_same = pairs
        .iter()
        .filter(|p| same(&p.0) && same(&p.1))
        .map(|p| p.2)
        .fold(0.0, f64::max);
    let best_diff = pairs
        .iter()
        .filter(|p| !(same(&p.0) && same(&p.1)))
        .map(|p| p.2)
        .fold(1.0, f64::min);
    eprintln!(
        "worst same-recording BER {worst_same:.3}, best different-recording BER {best_diff:.3}"
    );
    assert!(
        worst_same <= library::SAME_RECORDING_MAX_BER,
        "{worst_same}"
    );
    assert!(
        best_diff > library::SAME_RECORDING_MAX_BER + 0.05,
        "{best_diff}"
    );

    let groups = library::duplicate_groups(&conn, 50).unwrap();
    let acoustic: Vec<_> = groups
        .iter()
        .filter(|g| g.kind == "same-recording")
        .collect();
    assert_eq!(acoustic.len(), 1, "{acoustic:#?}");
    let mut members: Vec<String> = acoustic[0]
        .tracks
        .iter()
        .map(|t| t.artist.clone().unwrap())
        .collect();
    members.sort();
    assert_eq!(
        members,
        vec![
            "A-128.mp3",
            "A-320.mp3",
            "A-aac.m4a",
            "A-quieter.flac",
            "A-trimmed.flac",
            "A.flac"
        ]
    );
}
