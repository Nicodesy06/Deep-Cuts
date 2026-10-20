//! Default on-disk locations, shared by the desktop app and the CLI.
//! `DEEPCUTS_HOME` overrides the base directory (portable installs, tests).

use std::path::PathBuf;

pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("DEEPCUTS_HOME") {
        return PathBuf::from(h);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("DeepCuts")
}

pub fn db_path() -> PathBuf {
    home().join("library.db")
}

/// Where merged duplicate copies are moved (never deleted).
pub fn trash_dir() -> PathBuf {
    home().join("trash")
}

pub fn backups_dir() -> PathBuf {
    home().join("backups")
}
