use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Lower-case file extension without the dot.
pub fn extension(path: &std::path::Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "aif", "aiff", "flac", "m4a", "mp4", "aac", "alac", "ogg",
];

pub fn is_audio_file(path: &std::path::Path) -> bool {
    // Deep Cuts' own temporary files (staged tag writes, partial copies)
    // are never imported.
    let ours = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".deepcuts-"));
    !ours && extension(path).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

/// Begin a write transaction. IMMEDIATE takes the write lock up front, so
/// concurrent writers (UI + analysis workers) wait on the busy timeout
/// instead of failing with SQLITE_BUSY when a read lock would be upgraded.
pub fn write_tx(conn: &rusqlite::Connection) -> rusqlite::Result<rusqlite::Transaction<'_>> {
    rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
}

/// Run `f` atomically: in a new transaction when the connection is in
/// autocommit mode, or as part of the caller's already-open transaction.
pub fn atomic<T>(
    conn: &rusqlite::Connection,
    f: impl FnOnce(&rusqlite::Connection) -> crate::Result<T>,
) -> crate::Result<T> {
    if conn.is_autocommit() {
        let tx = write_tx(conn)?;
        let result = f(&tx).and_then(|v| {
            tx.commit()?;
            Ok(v)
        });
        // File moves done inside the transaction are kept on commit and
        // reversed on failure (see `fileops`).
        crate::fileops::settle(result.is_ok());
        result
    } else {
        f(conn)
    }
}
