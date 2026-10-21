//! Rekordbox My Tags carried in comments become Deep Cuts tags on import.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, library, rekordbox};

#[test]
fn my_tags_from_rekordbox_comments() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("m");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 2.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    let mut locs = Vec::new();
    for n in ["a.wav", "b.wav"] {
        let p = music.join(n);
        write_wav(&p, &audio, 22050);
        let p = library::path_key(&p);
        let url = format!(
            "file://localhost{}{}",
            if p.starts_with('/') { "" } else { "/" },
            p.replace('\\', "/").replace(' ', "%20")
        );
        locs.push(url);
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0"><PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
<COLLECTION Entries="2">
<TRACK TrackID="1" Name="A" Artist="X" Comments="/* Dub Techno / Warm Up */ deep one" Location="{}"/>
<TRACK TrackID="2" Name="B" Artist="Y" Comments="/* warm up */" Location="{}"/>
</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="0"></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
        locs[0], locs[1]
    );
    let xml_path = tmp.path().join("rb.xml");
    std::fs::write(&xml_path, xml).unwrap();
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let rep = rekordbox::import_xml(&mut conn, &xml_path).unwrap();
    let mt = rep.my_tags.unwrap();
    assert_eq!((mt.tracks, mt.tags, mt.assignments), (2, 2, 3), "{mt:?}");
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
    assert_eq!(q("tag:\"Warm Up\""), 2);
    assert_eq!(q("tag:\"Dub Techno\""), 1);
    // Both are children of "My Tag", so tag:"My Tag" finds them all.
    assert_eq!(q("tag:\"My Tag\""), 2);
    // Re-importing does not duplicate anything.
    let mt = rekordbox::import_xml(&mut conn, &xml_path)
        .unwrap()
        .my_tags
        .unwrap();
    assert_eq!((mt.tags, mt.assignments), (0, 0), "{mt:?}");
}
