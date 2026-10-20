//! Watch folders: new or changed audio files are imported automatically
//! (read-only, incremental). Removed/renamed files are marked missing so a
//! re-appearing file is relinked to its existing track.

use crate::AppState;
use dc_core::{db, jobs, library};
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const DEBOUNCE: Duration = Duration::from_secs(3);

/// (Re)create the OS watcher for the current watch-folder list.
pub fn restart(app: &AppHandle) {
    let state = app.state::<AppState>();
    let folders: Vec<String> = match state.conn.lock().map(|c| library::watch_folders(&c)) {
        Ok(Ok(f)) => f.into_iter().filter(|f| f.exists).map(|f| f.path).collect(),
        _ => return,
    };
    // Dropping the previous watcher closes its channel and ends its thread.
    *state.watcher.lock().unwrap() = None;
    if folders.is_empty() {
        return;
    }
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("watch: cannot start watcher: {e}");
            return;
        }
    };
    for f in &folders {
        if let Err(e) = watcher.watch(std::path::Path::new(f), RecursiveMode::Recursive) {
            eprintln!("watch: cannot watch {f}: {e}");
        }
    }
    *state.watcher.lock().unwrap() = Some(watcher);
    let app = app.clone();
    std::thread::Builder::new()
        .name("dc-watch".into())
        .spawn(move || {
            let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
            loop {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(Ok(ev)) => {
                        if matches!(
                            ev.kind,
                            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                        ) {
                            pending.extend(ev.paths);
                        }
                    }
                    Ok(Err(e)) => eprintln!("watch: {e}"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if !pending.is_empty() {
                            scan_now(&app, std::mem::take(&mut pending).into_iter().collect());
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .ok();
}

/// Import `paths` (files or folders) in the background.
pub fn scan_now(app: &AppHandle, paths: Vec<PathBuf>) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = (|| -> dc_core::Result<library::ImportReport> {
            let mut conn = db::open(&state.db_path)?;
            // Paths that vanished: mark their files missing (enables relinking).
            for p in paths.iter().filter(|p| !p.exists()) {
                conn.execute(
                    "UPDATE files SET missing = 1 WHERE path = ?1",
                    [p.to_string_lossy()],
                )?;
            }
            let existing: Vec<PathBuf> = paths
                .into_iter()
                .filter(|p| p.exists() && (p.is_dir() || dc_core::util::is_audio_file(p)))
                .collect();
            let rep = library::import_paths(&mut conn, &existing, None, &mut |_| {})?;
            let auto = library::get_setting(&conn, "autoAnalyze")?.as_deref() != Some("false");
            if auto && !rep.added_track_ids.is_empty() {
                jobs::enqueue_analysis(&conn, &rep.added_track_ids, false)?;
            }
            for f in library::watch_folders(&conn)? {
                library::mark_watch_scanned(&conn, &f.path)?;
            }
            Ok(rep)
        })();
        match result {
            Ok(rep) if rep.added + rep.updated + rep.relinked > 0 => {
                let _ = app.emit("library-changed", rep);
            }
            Ok(_) => {}
            Err(e) => eprintln!("watch: import failed: {e}"),
        }
    });
}

/// Startup: watch and catch up with changes made while the app was closed.
pub fn start(app: &AppHandle) {
    restart(app);
    let state = app.state::<AppState>();
    let folders: Vec<PathBuf> = state
        .conn
        .lock()
        .ok()
        .and_then(|c| library::watch_folders(&c).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.exists)
        .map(|f| PathBuf::from(f.path))
        .collect();
    if !folders.is_empty() {
        scan_now(app, folders);
    }
}
