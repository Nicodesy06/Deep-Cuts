//! Live lookups against the real catalogue APIs. Network-dependent, so
//! ignored by default: `cargo test -p dc-core --test autotag_live -- --ignored`.

use dc_core::autotag::{Http, Query, lookup};

fn q() -> Query {
    Query {
        artist: "Bicep".into(),
        title: "Glue".into(),
        version: String::new(),
        duration_s: Some(269.0),
        isrc: None,
    }
}

#[test]
#[ignore]
fn live_deezer_musicbrainz_itunes() {
    let http = Http::new(None);
    let (c, sc) = lookup(&http, "deezer", &q(), None)
        .unwrap()
        .expect("deezer match");
    eprintln!("deezer {sc} {c:?}");
    assert!(sc >= 0.85);
    assert_eq!(c.isrc.as_deref(), Some("GBCFB1700229"));
    assert_eq!(c.label.as_deref(), Some("Ninja Tune"));
    assert!(c.bpm.is_some());

    let (c, sc) = lookup(&http, "itunes", &q(), None)
        .unwrap()
        .expect("itunes match");
    eprintln!("itunes {sc} {c:?}");
    assert!(sc >= 0.85);
    assert_eq!(c.year, Some(2017));
    assert!(c.artwork_url.as_deref().unwrap_or("").contains("1000x1000"));
    assert!(!c.compilation, "a compilation was preferred: {c:?}");

    let r = lookup(&http, "musicbrainz", &q(), None).unwrap();
    eprintln!("musicbrainz {r:?}");
    let (c, _) = r.expect("musicbrainz candidate");
    assert_eq!(c.artist, "Bicep");
    assert!(c.label.is_some(), "label from the release lookup: {c:?}");

    // Discogs needs the user's token.
    assert!(lookup(&http, "discogs", &q(), None).is_err());
}

mod common;

#[test]
#[ignore]
fn live_cover_art_into_a_flac() {
    use common::*;
    use dc_core::autotag::{self, SourcedProposal};
    use dc_core::{db, library};
    if !has_ffmpeg() {
        return;
    }
    let http = Http::new(None);
    let (c, _) = lookup(&http, "itunes", &q(), None).unwrap().unwrap();
    let url = c.artwork_url.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    let wav = tmp.path().join("s.wav");
    write_wav(
        &wav,
        &synth(
            &Spec {
                bpm: 124.0,
                tonic: 0,
                minor: false,
                seconds: 2.0,
                first_beat_s: 0.0,
            },
            44100,
        ),
        44100,
    );
    assert!(ffmpeg_encode(
        &wav,
        &music.join("a.flac"),
        &["-c:a", "flac"],
        &[]
    ));
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id: String = conn
        .query_row("SELECT id FROM tracks", [], |r| r.get(0))
        .unwrap();
    let p = SourcedProposal {
        track_id: id.clone(),
        field: "artwork".into(),
        old: String::new(),
        new: url,
        provider: "itunes".into(),
        score: 1.0,
    };
    let (_, art) = autotag::apply(&mut conn, &http, &[p], Some(&tmp.path().join("bak"))).unwrap();
    assert_eq!((art.written, art.failed.len()), (1, 0), "{art:?}");
    let has: bool = conn
        .query_row("SELECT has_artwork FROM tracks WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(has);
}
