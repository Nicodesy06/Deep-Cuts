//! Default on-disk locations, shared by the desktop app and the CLI.
//! `CRADEX_HOME` (or the older `DEEPCUTS_HOME`) overrides the base directory
//! (portable installs, tests).

use std::path::{Path, PathBuf};

/// Read a setting from the environment under its current name, falling back
/// to the name used before the app was renamed from Cradex.
pub fn env_var(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("CRADEX_{name}"))
        .or_else(|| std::env::var_os(format!("DEEPCUTS_{name}")))
}

pub fn home() -> PathBuf {
    if let Some(h) = env_var("HOME") {
        return PathBuf::from(h);
    }
    home_in(&dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// The app was called Cradex before 0.3: a library created then stays in
/// its `DeepCuts` folder (nothing is moved), new installs use `Cradex`.
fn home_in(base: &Path) -> PathBuf {
    let current = base.join("Cradex");
    let legacy = base.join("DeepCuts");
    if !current.exists() && legacy.join("library.db").exists() {
        legacy
    } else {
        current
    }
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

#[cfg(test)]
mod tests {
    use super::home_in;

    #[test]
    fn existing_deep_cuts_library_is_kept_in_place() {
        let base = tempfile::tempdir().unwrap();
        assert_eq!(home_in(base.path()), base.path().join("Cradex"));
        // An empty old folder is not a library.
        std::fs::create_dir_all(base.path().join("DeepCuts")).unwrap();
        assert_eq!(home_in(base.path()), base.path().join("Cradex"));
        std::fs::write(base.path().join("DeepCuts/library.db"), b"").unwrap();
        assert_eq!(home_in(base.path()), base.path().join("DeepCuts"));
        // Once a Cradex folder exists it wins.
        std::fs::create_dir_all(base.path().join("Cradex")).unwrap();
        assert_eq!(home_in(base.path()), base.path().join("Cradex"));
    }
}
