//! Rekordbox My Tags carried in comments become Deep Cuts tags on import.

mod common;

use common::*;
use dc_core::query::{TrackQuery, query_tracks};
use dc_core::{db, djlib, library, m3uimport, mytag, rekordbox};

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

#[test]
fn my_tags_from_file_comments_m3u8_import_and_tag_playlists() {
    if !has_ffmpeg() {
        eprintln!("ffmpeg not available: skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let spec = Spec {
        bpm: 124.0,
        tonic: 0,
        minor: false,
        seconds: 3.0,
        first_beat_s: 0.0,
    };
    let wav = make_track(tmp.path(), "src.wav", &spec);
    let music = tmp.path().join("music");
    let more = tmp.path().join("more");
    std::fs::create_dir_all(&music).unwrap();
    std::fs::create_dir_all(more.join("sub")).unwrap();
    let flac = ["-c:a", "flac"];
    assert!(ffmpeg_encode(
        &wav,
        &music.join("a.flac"),
        &flac,
        &[
            ("comment", "/* Peak Time / Dark */ big room"),
            ("title", "A")
        ]
    ));
    assert!(ffmpeg_encode(
        &wav,
        &music.join("b.flac"),
        &flac,
        &[("title", "B")]
    ));
    assert!(ffmpeg_encode(
        &wav,
        &more.join("sub").join("c.flac"),
        &flac,
        &[("comment", "/* dark */"), ("title", "C")]
    ));

    // Folder import reads the My Tags in the file comments.
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    let rep =
        library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let mt = rep.my_tags.unwrap();
    assert_eq!((mt.tracks, mt.tags, mt.assignments), (1, 2, 2), "{mt:?}");
    let q = |conn: &rusqlite::Connection, s: &str| {
        query_tracks(
            conn,
            &TrackQuery {
                query: s.into(),
                ..Default::default()
            },
        )
        .unwrap()
        .total
    };
    assert_eq!(q(&conn, "tag:Dark"), 1);
    assert_eq!(q(&conn, "tag:\"My Tag\""), 1);

    // An M3U8 (relative path, a library track, a missing file) becomes a
    // playlist; the new file is imported and its My Tag read.
    let list = more.join("Friday.m3u8");
    std::fs::write(
        &list,
        format!(
            "#EXTM3U\n#EXTINF:3,X - C\nsub/c.flac\n{}\nmissing.mp3\n",
            music.join("a.flac").display()
        ),
    )
    .unwrap();
    let r = m3uimport::import(&mut conn, &list).unwrap();
    assert_eq!((r.entries, r.in_library, r.imported), (3, 1, 1), "{r:?}");
    assert_eq!(r.missing.len(), 1);
    assert_eq!(r.name, "Friday");
    let pl = r.playlist_id.unwrap();
    let ids = library::playlist_track_ids(&conn, &pl).unwrap();
    let titles: Vec<String> = ids
        .iter()
        .map(|i| library::get_track(&conn, i).unwrap().title.unwrap())
        .collect();
    assert_eq!(titles, vec!["C", "A"]);
    assert_eq!(q(&conn, "tag:Dark"), 2);

    // One playlist per My Tag: M3U8 files and a "My Tag" folder in DJ exports.
    let out = tmp.path().join("out");
    let files = mytag::export_m3u8(&conn, &out).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["My Tag - Dark.m3u8", "My Tag - Peak Time.m3u8"]);
    let dark = std::fs::read_to_string(&files[0]).unwrap();
    assert!(dark.contains("c.flac") && dark.contains("a.flac"), "{dark}");
    let lib = djlib::whole_library(&conn).unwrap();
    let p = lib
        .playlists
        .iter()
        .find(|p| p.folders == ["My Tag"] && p.name == "Dark")
        .unwrap();
    assert_eq!(p.tracks.len(), 2);

    // Scanning the whole library again adds nothing new.
    let again = mytag::scan_library(&conn).unwrap();
    assert_eq!(again.assignments, 0, "{again:?}");
}
