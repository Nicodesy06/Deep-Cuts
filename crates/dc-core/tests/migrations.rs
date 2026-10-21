//! Schema upgrades (audit DC-08, DB-001): a library created at every older
//! schema version upgrades to the current one with its tracks, files,
//! decisions, tags, playlists and track ids intact, and a copy of the old
//! database is kept.

use dc_core::db;
use rusqlite::Connection;

/// Library content using only tables that exist since schema v1.
fn seed(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO tracks (id, created_at, updated_at, title, artist, bpm, key_camelot, rating)
           VALUES ('t1', 1, 1, 'One', 'Artist A', 124.0, '8A', 4),
                  ('t2', 2, 2, 'Two', 'Artist B', 126.5, '9A', NULL);
         INSERT INTO files (id, track_id, path, size, quick_hash, missing, last_seen, added_at)
           VALUES ('f1', 't1', '/music/one.mp3', 1000, 'h1', 0, 1, 1),
                  ('f2', 't2', '/music/two.flac', 2000, 'h2', 1, 1, 1);
         INSERT INTO decisions (track_id, context, decision, updated_at)
           VALUES ('t1', 'global', 'keep', 1), ('t2', 'global', 'pass', 1);
         INSERT INTO tags (id, name, category) VALUES ('g1', 'Dark', 'custom');
         INSERT INTO track_tags (track_id, tag_id, origin, status, updated_at)
           VALUES ('t1', 'g1', 'user', 'confirmed', 1);
         INSERT INTO playlists (id, name, kind, created_at, updated_at)
           VALUES ('p1', 'Friday', 'manual', 1, 1);
         INSERT INTO playlist_tracks (playlist_id, position, track_id)
           VALUES ('p1', 0, 't2'), ('p1', 1, 't1');",
    )
    .unwrap();
}

fn snapshot(conn: &Connection) -> Vec<String> {
    let q = |sql: &str| -> Vec<String> {
        let mut st = conn.prepare(sql).unwrap();
        st.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    let mut out = Vec::new();
    out.extend(q("SELECT id || '|' || ifnull(title,'') || '|' || ifnull(artist,'') || '|' || ifnull(bpm,'') || '|' || ifnull(key_camelot,'') || '|' || ifnull(rating,'') FROM tracks ORDER BY id"));
    out.extend(q("SELECT id || '|' || track_id || '|' || path || '|' || size || '|' || missing FROM files ORDER BY id"));
    out.extend(q(
        "SELECT track_id || '|' || context || '|' || decision FROM decisions ORDER BY track_id",
    ));
    out.extend(q("SELECT track_id || '|' || tag_id || '|' || origin || '|' || status FROM track_tags ORDER BY track_id"));
    out.extend(q("SELECT playlist_id || '|' || position || '|' || track_id FROM playlist_tracks ORDER BY playlist_id, position"));
    out
}

#[test]
fn every_old_schema_upgrades_with_data_intact() {
    for start in 1..db::SCHEMA_VERSION {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lib.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
            db::migrate_to(&conn, start).unwrap();
            assert_eq!(db::schema_version(&conn).unwrap(), start);
            seed(&conn);
        }
        let before = {
            let conn = Connection::open(&path).unwrap();
            snapshot(&conn)
        };
        let conn = db::open(&path).unwrap();
        assert_eq!(
            db::schema_version(&conn).unwrap(),
            db::SCHEMA_VERSION,
            "from v{start}"
        );
        assert_eq!(
            snapshot(&conn),
            before,
            "data changed upgrading from v{start}"
        );
        assert_eq!(db::integrity_check(&conn).unwrap(), vec!["ok".to_string()]);
        let fk: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            fk, 0,
            "foreign key violations after upgrading from v{start}"
        );
        // The library still works through the normal API.
        let rows = dc_core::query::query_tracks(&conn, &Default::default()).unwrap();
        assert_eq!(rows.tracks.len(), 2, "from v{start}");
        // A copy of the old database was kept, at the old version.
        let bak = tmp.path().join(format!("lib.db.pre-v{start}.bak"));
        let old = Connection::open(&bak).unwrap();
        assert_eq!(db::schema_version(&old).unwrap(), start);
    }
}

#[test]
fn newer_schema_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("lib.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", db::SCHEMA_VERSION + 1)
            .unwrap();
    }
    let err = db::open(&path).unwrap_err();
    assert!(err.to_string().contains("newer than this build"), "{err}");
}
