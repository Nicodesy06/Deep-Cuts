//! SQLite connection management and schema migrations.

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

/// Ordered list of migrations; index + 1 == schema version (PRAGMA user_version).
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_init.sql"),
    include_str!("migrations/0002_phase2.sql"),
    include_str!("migrations/0003_sets_features.sql"),
    include_str!("migrations/0004_bandwidth.sql"),
    include_str!("migrations/0005_fingerprints.sql"),
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

pub fn migrate(conn: &Connection) -> Result<()> {
    let current = schema_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(crate::Error::Invalid(format!(
            "database schema v{current} is newer than this build supports (v{SCHEMA_VERSION})"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = crate::util::write_tx(conn)?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn integrity_check(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
