//! Offline playlist matcher: external list → local tracks → playlist.

mod common;

use common::*;
use dc_core::{db, library, playlist_match};

#[test]
fn external_playlist_matches_library_and_lists_missing() {
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
    let meta = [
        ("a.wav", "Bicep", "Glue", ""),
        ("b.wav", "Daft Punk", "One More Time", ""),
        ("c.wav", "Daft Punk", "One More Time", "Radio Edit"),
        ("d.wav", "Röyksopp", "Eple", ""),
    ];
    for (f, ..) in &meta {
        write_wav(&music.join(f), &audio, 22050);
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    for (f, artist, title, version) in &meta {
        conn.execute(
            "UPDATE tracks SET artist = ?1, title = ?2, version = nullif(?3, '')
             WHERE id = (SELECT track_id FROM files WHERE path LIKE '%' || ?4)",
            (artist, title, version, f),
        )
        .unwrap();
    }
    let id_of = |f: &str| -> String {
        conn.query_row(
            "SELECT track_id FROM files WHERE path LIKE '%' || ?1",
            [f],
            |r| r.get(0),
        )
        .unwrap()
    };
    let d_path: String = conn
        .query_row("SELECT path FROM files WHERE path LIKE '%d.wav'", [], |r| {
            r.get(0)
        })
        .unwrap();

    let list = tmp.path().join("Friday.csv");
    std::fs::write(
        &list,
        "Track Name,Artist Name(s),Duration (ms)\n\
         Glue,Bicep,\n\
         One More Time - Radio Edit,Daft Punk,\n\
         Something We Don't Have,Nobody,\n",
    )
    .unwrap();
    let rep = playlist_match::read_and_match(&conn, &list).unwrap();
    assert_eq!(rep.name, "Friday");
    assert_eq!((rep.matched, rep.missing), (2, 1), "{:#?}", rep.rows);
    assert_eq!(rep.rows[0].candidates[0].track.id, id_of("a.wav"));
    // The radio edit, not the original.
    assert_eq!(rep.rows[1].candidates[0].track.id, id_of("c.wav"));
    assert_eq!(rep.rows[2].status, "missing");

    // M3U from another machine: exact path wins, otherwise the display text.
    let m3u = tmp.path().join("x.m3u8");
    std::fs::write(
        &m3u,
        format!("#EXTM3U\n{d_path}\n#EXTINF:200,ROYKSOPP - eple\n/other/pc/eple.mp3\n"),
    )
    .unwrap();
    let rep = playlist_match::read_and_match(&conn, &m3u).unwrap();
    assert_eq!(rep.matched, 2, "{:#?}", rep.rows);
    assert!(
        rep.rows
            .iter()
            .all(|r| r.candidates[0].track.id == id_of("d.wav"))
    );

    // Confirmed tracks become a playlist, without repeats.
    let ids = vec![id_of("a.wav"), id_of("c.wav"), id_of("a.wav")];
    let pl = playlist_match::create_playlist(&conn, "Friday", &ids).unwrap();
    assert_eq!(
        library::playlist_track_ids(&conn, &pl).unwrap(),
        vec![id_of("a.wav"), id_of("c.wav")]
    );
}
