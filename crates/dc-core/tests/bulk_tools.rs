//! Find & replace and playlist tools.

use dc_core::bulk::{self, FindReplace};
use dc_core::playlist_tools::{self, Combine};
use dc_core::{db, history, library};

fn track(conn: &rusqlite::Connection, title: &str, artist: &str, genre: &str) -> String {
    let id = dc_core::util::new_id();
    conn.execute(
        "INSERT INTO tracks (id, created_at, updated_at) VALUES (?1, 0, 0)",
        [&id],
    )
    .unwrap();
    library::edit_field(conn, std::slice::from_ref(&id), "title", Some(title)).unwrap();
    library::edit_field(conn, std::slice::from_ref(&id), "artist", Some(artist)).unwrap();
    library::edit_field(conn, std::slice::from_ref(&id), "genre", Some(genre)).unwrap();
    id
}

fn fr(fields: &[&str], find: &str, replace: &str) -> FindReplace {
    FindReplace {
        fields: fields.iter().map(|s| s.to_string()).collect(),
        find: find.into(),
        replace: replace.into(),
        case_sensitive: false,
        whole_word: false,
        regex: false,
    }
}

#[test]
fn find_replace_preview_apply_undo() {
    let conn = db::open_in_memory().unwrap();
    let a = track(&conn, "Strobe (Original Mix)", "deadmau5", "Prog House");
    let b = track(&conn, "Opus (original mix)", "Eric Prydz", "prog house");
    let c = track(&conn, "Housework", "Someone", "House");
    let ids = vec![a.clone(), b.clone(), c.clone()];

    // plain, case-insensitive
    let p = bulk::preview(
        &conn,
        &ids,
        &fr(&["genre"], "prog house", "Progressive House"),
    )
    .unwrap();
    assert_eq!(p.len(), 2);
    assert!(
        p.iter()
            .all(|x| x.new.as_deref() == Some("Progressive House"))
    );
    // whole word: "House" must not match inside "Housework"
    let mut w = fr(&["title"], "house", "Home");
    w.whole_word = true;
    assert!(bulk::preview(&conn, &ids, &w).unwrap().is_empty());
    // regex with capture groups, case sensitive
    let mut r = fr(&["title"], r"\s*\((?i:original mix)\)$", "");
    r.regex = true;
    r.case_sensitive = true;
    let p2 = bulk::preview(&conn, &ids, &r).unwrap();
    assert_eq!(p2.len(), 2);
    assert_eq!(p2[0].new.as_deref(), Some("Strobe"));
    // replacement text is literal unless regex is on
    let lit = bulk::preview(&conn, &[c.clone()], &fr(&["artist"], "Someone", "$1 Else")).unwrap();
    assert_eq!(lit[0].new.as_deref(), Some("$1 Else"));
    // errors are explicit
    assert!(bulk::preview(&conn, &ids, &fr(&["bpm"], "1", "2")).is_err());
    let mut bad = fr(&["title"], "(", "");
    bad.regex = true;
    assert!(bulk::preview(&conn, &ids, &bad).is_err());

    assert_eq!(bulk::apply(&conn, &p).unwrap(), 2);
    assert_eq!(
        library::get_track(&conn, &b).unwrap().genre.as_deref(),
        Some("Progressive House")
    );
    let u = history::undo(&conn).unwrap().unwrap();
    assert_eq!(u.action, "find-replace");
    assert_eq!(
        library::get_track(&conn, &b).unwrap().genre.as_deref(),
        Some("prog house")
    );
}

#[test]
fn playlist_combine_shuffle_prefix_occurrences() {
    let conn = db::open_in_memory().unwrap();
    let t: Vec<String> = (0..6)
        .map(|i| track(&conn, &format!("T{i}"), "A", "G"))
        .collect();
    let p1 = library::create_playlist(&conn, "Warm-up", None, "manual", None).unwrap();
    let p2 = library::create_playlist(&conn, "Peak", None, "manual", None).unwrap();
    library::set_playlist_tracks(&conn, &p1, &t[0..4]).unwrap();
    library::set_playlist_tracks(&conn, &p2, &[t[2].clone(), t[3].clone(), t[4].clone()]).unwrap();
    let both = vec![p1.clone(), p2.clone()];
    assert_eq!(
        playlist_tools::combined_ids(&conn, &both, Combine::Union).unwrap(),
        t[0..5].to_vec()
    );
    assert_eq!(
        playlist_tools::combined_ids(&conn, &both, Combine::Intersect).unwrap(),
        t[2..4].to_vec()
    );
    assert_eq!(
        playlist_tools::combined_ids(&conn, &both, Combine::Subtract).unwrap(),
        t[0..2].to_vec()
    );
    let new = playlist_tools::combine(&conn, "Common", &both, Combine::Intersect).unwrap();
    assert_eq!(
        library::playlist_track_ids(&conn, &new).unwrap(),
        t[2..4].to_vec()
    );
    // sources unchanged
    assert_eq!(
        library::playlist_track_ids(&conn, &p1).unwrap(),
        t[0..4].to_vec()
    );

    playlist_tools::shuffle(&conn, &p1, 42).unwrap();
    let shuffled = library::playlist_track_ids(&conn, &p1).unwrap();
    let mut sorted = shuffled.clone();
    sorted.sort();
    let mut orig = t[0..4].to_vec();
    orig.sort();
    assert_eq!(sorted, orig, "same tracks");
    assert_ne!(shuffled, t[0..4].to_vec(), "order changed for this seed");
    history::undo(&conn).unwrap();
    assert_eq!(
        library::playlist_track_ids(&conn, &p1).unwrap(),
        t[0..4].to_vec()
    );

    assert_eq!(
        playlist_tools::add_prefix(&conn, &both, "FRI · ").unwrap(),
        2
    );
    assert_eq!(
        playlist_tools::add_prefix(&conn, &both, "FRI · ").unwrap(),
        0,
        "idempotent"
    );
    let occ = playlist_tools::occurrences(&conn, &t[3]).unwrap();
    let names: Vec<&str> = occ.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, vec!["Common", "FRI · Peak", "FRI · Warm-up"]);
    assert_eq!(occ[1].positions, vec![2]);
}
