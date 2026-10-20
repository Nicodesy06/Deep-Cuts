//! Database backups (SQLite online backup API), restore and listing.
//!
//! Backups cover the Deep Cuts database (metadata, decisions, playlists,
//! cues, analysis) — NOT the audio files themselves.

use crate::error::{Error, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    pub created_at: i64,
    pub reason: String,
}

fn sanitize(reason: &str) -> String {
    let s: String = reason
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').chars().take(40).collect()
}

/// Create a consistent snapshot of the open database into `dir`.
pub fn create(conn: &Connection, dir: &Path, reason: &str) -> Result<BackupInfo> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let name = format!("deepcuts-{stamp}-{}.db", sanitize(reason));
    let path = dir.join(&name);
    {
        let mut dst = Connection::open(&path)?;
        let backup = rusqlite::backup::Backup::new(conn, &mut dst)?;
        backup.run_to_completion(256, Duration::from_millis(5), None)?;
    }
    let check = Connection::open(&path)?;
    let ok: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(Error::Invalid(format!(
            "backup failed integrity check: {ok}"
        )));
    }
    Ok(BackupInfo {
        size: std::fs::metadata(&path)?.len(),
        path: path.to_string_lossy().into_owned(),
        file_name: name,
        created_at: crate::util::now_ms(),
        reason: reason.into(),
    })
}

pub fn list(dir: &Path) -> Result<Vec<BackupInfo>> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("deepcuts-") && name.ends_with(".db")) {
            continue;
        }
        let meta = e.metadata()?;
        let created_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // deepcuts-YYYYmmdd-HHMMSS.mmm-<reason>.db
        let reason = name
            .trim_end_matches(".db")
            .splitn(4, '-')
            .nth(3)
            .unwrap_or("")
            .to_string();
        out.push(BackupInfo {
            path: p.to_string_lossy().into_owned(),
            file_name: name,
            size: meta.len(),
            created_at,
            reason,
        });
    }
    out.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    Ok(out)
}

/// Restore `backup` into the live database `conn` (in place, via the backup
/// API). A safety snapshot of the current state is taken first.
pub fn restore(conn: &mut Connection, backup: &Path, dir: &Path) -> Result<BackupInfo> {
    let src = Connection::open(backup)?;
    let ok: String = src.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ok != "ok" {
        return Err(Error::Invalid(format!("backup is damaged: {ok}")));
    }
    let v: i64 = src.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v > crate::db::SCHEMA_VERSION {
        return Err(Error::Invalid(
            "backup comes from a newer Deep Cuts version".into(),
        ));
    }
    let safety = create(conn, dir, "before-restore")?;
    {
        let backup = rusqlite::backup::Backup::new(&src, conn)?;
        backup.run_to_completion(256, Duration::from_millis(5), None)?;
    }
    crate::db::migrate(conn)?;
    Ok(safety)
}

/// Keep the newest `keep` automatic backups (reason starting with "auto").
pub fn prune_auto(dir: &Path, keep: usize) -> Result<usize> {
    let autos: Vec<BackupInfo> = list(dir)?
        .into_iter()
        .filter(|b| b.reason.starts_with("auto"))
        .collect();
    let mut removed = 0;
    for b in autos.into_iter().skip(keep) {
        std::fs::remove_file(PathBuf::from(&b.path))?;
        removed += 1;
    }
    Ok(removed)
}
