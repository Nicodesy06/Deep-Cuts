//! Export field mapping, set chapters as crates, smart crates as VirtualDJ
//! filters.

mod common;

use common::*;
use dc_core::djlib::{self, ExportOptions, FieldMapping};
use dc_core::{db, library, sets, tags};

#[test]
fn mapping_chapters_and_virtualdj_filters() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 3.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    for n in ["a.wav", "b.wav", "c.wav", "d.wav"] {
        write_wav(&music.join(n), &audio, 22050);
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let id = |n: &str| -> String {
        conn.query_row(
            "SELECT track_id FROM files WHERE path LIKE '%' || ?1",
            [n],
            |r| r.get(0),
        )
        .unwrap()
    };
    let ids: Vec<String> = ["a.wav", "b.wav", "c.wav", "d.wav"]
        .iter()
        .map(|n| id(n))
        .collect();
    let a = ids[0].clone();
    for (f, v) in [
        ("comment", "warm-up"),
        ("genre", "Techno"),
        ("subgenre", "Hypnotic"),
        ("key", "Am"),
    ] {
        library::edit_field(&conn, std::slice::from_ref(&a), f, Some(v)).unwrap();
    }
    let dark = tags::create(&conn, "Dark", "mood", None).unwrap();
    let peak = tags::create(&conn, "Peak", "custom", None).unwrap();
    tags::assign(&conn, std::slice::from_ref(&a), &dark, true).unwrap();
    tags::assign(&conn, std::slice::from_ref(&a), &peak, true).unwrap();

    let set = library::create_playlist(&conn, "Friday", None, "set", None).unwrap();
    library::add_to_playlist(&conn, &set, &ids).unwrap();
    sets::set_chapter(&conn, &set, 0, "opening", None).unwrap();
    sets::set_chapter(&conn, &set, 2, "peak", Some("Main room")).unwrap();
    library::create_playlist(
        &conn,
        "Techno 124",
        None,
        "smart",
        Some("genre:techno bpm:120-130"),
    )
    .unwrap();
    library::create_playlist(&conn, "Dark ones", None, "smart", Some("tag:Dark")).unwrap();

    // Default mapping: comment unchanged.
    let lib = djlib::whole_library(&conn).unwrap();
    let ta = lib.tracks.iter().find(|t| t.row.id == a).unwrap();
    assert_eq!(ta.row.comment.as_deref(), Some("warm-up"));
    assert_eq!(ta.row.genre.as_deref(), Some("Techno"));

    let opts = ExportOptions {
        mapping: FieldMapping {
            comment: vec!["comment".into(), "tags".into(), "key".into()],
            genre_with_subgenre: true,
        },
        chapters: true,
    };
    let lib = djlib::whole_library_with(&conn, opts).unwrap();
    let ta = lib.tracks.iter().find(|t| t.row.id == a).unwrap();
    assert_eq!(
        ta.row.comment.as_deref(),
        Some("warm-up · /* Dark / Peak */ · 8A")
    );
    assert_eq!(ta.row.genre.as_deref(), Some("Techno / Hypnotic"));
    let names: Vec<(String, usize)> = lib
        .playlists
        .iter()
        .filter(|p| p.folders == vec!["Friday – chapters".to_string()])
        .map(|p| (p.name.clone(), p.tracks.len()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("01 opening".to_string(), 2),
            ("02 peak – Main room".to_string(), 2)
        ]
    );
    assert!(
        lib.playlists
            .iter()
            .any(|p| p.name == "Friday" && p.tracks.len() == 4)
    );
    assert_eq!(lib.smart.len(), 2);

    // The mapped comment reaches the Rekordbox XML.
    let xml_path = tmp.path().join("rb.xml");
    djlib::export_to("rekordbox", &lib, &xml_path).unwrap();
    let xml = std::fs::read_to_string(&xml_path).unwrap();
    assert!(xml.contains("warm-up · /* Dark / Peak */ · 8A"), "{xml}");

    // VirtualDJ: snapshots plus the translated filters.
    let vdj = tmp.path().join("vdj");
    djlib::export_to("virtualdj", &lib, &vdj).unwrap();
    let txt = std::fs::read_to_string(vdj.join("Smart Crates - VirtualDJ filters.txt")).unwrap();
    assert!(
        txt.contains("genre contains \"techno\" and (bpm>=120 and bpm<=130)"),
        "{txt}"
    );
    assert!(
        txt.contains("no VirtualDJ equivalent for: tag:Dark"),
        "{txt}"
    );
    assert!(
        vdj.join("Smart Crates")
            .join("Techno 124.vdjfolder")
            .exists()
    );
}

#[test]
fn html_and_pdf_track_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir_all(&music).unwrap();
    let audio = synth(
        &Spec {
            bpm: 124.0,
            tonic: 0,
            minor: false,
            seconds: 3.0,
            first_beat_s: 0.0,
        },
        22050,
    );
    for n in ["a.wav", "b.wav", "c.wav"] {
        write_wav(&music.join(n), &audio, 22050);
    }
    let mut conn = db::open(&tmp.path().join("lib.db")).unwrap();
    library::import_paths(&mut conn, std::slice::from_ref(&music), None, &mut |_| {}).unwrap();
    let ids: Vec<String> = {
        let mut st = conn
            .prepare("SELECT track_id FROM files ORDER BY path")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    library::edit_field(
        &conn,
        std::slice::from_ref(&ids[0]),
        "artist",
        Some("Röyksopp <&>"),
    )
    .unwrap();
    library::edit_field(&conn, std::slice::from_ref(&ids[0]), "bpm", Some("124")).unwrap();
    let set = library::create_playlist(&conn, "Friday", None, "set", None).unwrap();
    library::add_to_playlist(&conn, &set, &ids).unwrap();
    sets::set_chapter(&conn, &set, 0, "opening", None).unwrap();
    sets::set_chapter(&conn, &set, 1, "peak", Some("Main room")).unwrap();

    let html = tmp.path().join("set.html");
    let n = dc_core::printable::export(&conn, &ids, "Friday", Some(&set), "html", &html).unwrap();
    assert_eq!(n, 3);
    let h = std::fs::read_to_string(&html).unwrap();
    assert!(
        h.contains("<h2>opening</h2>") && h.contains("<h2>peak – Main room</h2>"),
        "{h}"
    );
    assert!(h.contains("Röyksopp &lt;&amp;&gt;"));
    assert!(h.contains("3 tracks"));

    let pdf = tmp.path().join("set.pdf");
    dc_core::printable::export(&conn, &ids, "Friday", Some(&set), "pdf", &pdf).unwrap();
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    if let Ok(out) = std::process::Command::new("pdftotext")
        .arg(&pdf)
        .arg("-")
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("Friday") && text.contains("Main room"),
            "{text}"
        );
        assert!(text.contains("Royksopp <&>"), "{text}");
    }
}
