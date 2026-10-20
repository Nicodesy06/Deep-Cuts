//! Deep Cuts desktop shell: exposes dc-core to the React UI over Tauri IPC
//! and serves library audio to the webview through the `dcaudio` protocol.

mod audio_protocol;
mod commands;
mod watch;

use dc_core::rusqlite::Connection;
use dc_core::{backup, db, jobs, library, paths};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub db_path: PathBuf,
    pub backups_dir: PathBuf,
    /// Connection for short UI requests. Long operations open their own.
    pub conn: Mutex<Connection>,
    pub pool: Mutex<Option<jobs::WorkerPool>>,
    pub import_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub audio: audio_protocol::AudioCache,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl AppState {
    pub fn start_pool(&self, app: &tauri::AppHandle, profile: &str) {
        let mut pool = self.pool.lock().unwrap();
        if let Some(p) = pool.take() {
            p.shutdown();
        }
        let handle = app.clone();
        *pool = Some(jobs::WorkerPool::start(
            self.db_path.clone(),
            jobs::workers_for_profile(profile),
            Arc::new(move |ev: jobs::JobEvent| {
                let _ = handle.emit("job-event", ev);
            }),
        ));
    }

    pub fn stop_pool(&self) {
        if let Some(p) = self.pool.lock().unwrap().take() {
            p.shutdown();
        }
    }
}

/// Daily automatic database snapshot (keeps the 10 most recent).
fn auto_backup(conn: &Connection, dir: &std::path::Path) {
    let day = 24 * 3600 * 1000;
    let last = backup::list(dir)
        .ok()
        .and_then(|l| l.into_iter().find(|b| b.reason.starts_with("auto")))
        .map(|b| b.created_at)
        .unwrap_or(0);
    let has_tracks: i64 = conn
        .query_row("SELECT count(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0);
    if has_tracks > 0 && dc_core::util::now_ms() - last > day {
        match backup::create(conn, dir, "auto-daily") {
            Ok(_) => {
                let _ = backup::prune_auto(dir, 10);
            }
            Err(e) => eprintln!("automatic backup failed: {e}"),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_path = paths::db_path();
    let backups_dir = paths::backups_dir();
    let conn = db::open(&db_path)
        .unwrap_or_else(|e| panic!("cannot open library {}: {e}", db_path.display()));
    if let Err(e) = jobs::recover_interrupted(&conn) {
        eprintln!("job recovery failed: {e}");
    }
    auto_backup(&conn, &backups_dir);
    let profile = library::get_setting(&conn, "analysisProfile")
        .ok()
        .flatten()
        .unwrap_or_else(|| "balanced".into());
    let state = AppState {
        db_path: db_path.clone(),
        backups_dir,
        conn: Mutex::new(conn),
        pool: Mutex::new(None),
        import_cancel: Mutex::new(None),
        audio: audio_protocol::AudioCache::new(&db_path),
        watcher: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .register_asynchronous_uri_scheme_protocol("dcaudio", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                responder.respond(state.audio.handle(&request));
            });
        })
        .setup(move |app| {
            let state = app.state::<AppState>();
            state.start_pool(app.handle(), &profile);
            watch::start(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::library_stats,
            commands::query_tracks,
            commands::track_detail,
            commands::waveform,
            commands::set_decision,
            commands::edit_field,
            commands::revert_field,
            commands::set_notes,
            commands::set_favorite,
            commands::import_paths,
            commands::cancel_import,
            commands::import_rekordbox,
            commands::export_tracks,
            commands::list_playlists,
            commands::create_playlist,
            commands::rename_playlist,
            commands::delete_playlist,
            commands::add_to_playlist,
            commands::remove_from_playlist,
            commands::set_playlist_order,
            commands::enqueue_analysis,
            commands::queue_summary,
            commands::recent_jobs,
            commands::cancel_queued,
            commands::retry_failed,
            commands::clear_finished,
            commands::set_paused,
            commands::set_profile,
            commands::create_backup,
            commands::list_backups,
            commands::restore_backup,
            commands::check_missing,
            commands::duplicate_groups,
            commands::integrity_check,
            commands::get_settings,
            commands::set_setting,
            commands::validate_query,
            commands::undo,
            commands::redo,
            commands::recent_changes,
            commands::list_tags,
            commands::create_tag,
            commands::update_tag,
            commands::delete_tag,
            commands::add_tag_synonym,
            commands::remove_tag_synonym,
            commands::set_quick_tag,
            commands::tag_tracks,
            commands::quick_tag,
            commands::review_tag_suggestion,
            commands::fix_rules,
            commands::preview_fixes,
            commands::apply_fixes,
            commands::add_cue,
            commands::accept_cue,
            commands::delete_cue,
            commands::rename_cue,
            commands::plan_tag_write,
            commands::write_tags,
            commands::list_watch_folders,
            commands::add_watch_folder,
            commands::remove_watch_folder,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Interrupted analyses are re-queued on next start.
                window.state::<AppState>().stop_pool();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Deep Cuts");
}
