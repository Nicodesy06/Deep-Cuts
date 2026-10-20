//! IPC commands. Thin wrappers over dc-core; errors are returned as strings
//! for display in the UI.

use crate::AppState;
use dc_core::query::{TrackPage, TrackQuery};
use dc_core::{analysis, backup, db, export, jobs, library, rekordbox, stats};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

macro_rules! with_conn {
    ($state:expr, |$c:ident| $body:expr) => {{
        let $c = $state.conn.lock().map_err(e)?;
        #[allow(clippy::redundant_closure_call)]
        let r: dc_core::Result<_> = (|| $body)();
        r.map_err(e)
    }};
}

/// Run a blocking job on its own connection off the UI thread.
async fn blocking<T: Send + 'static>(
    db_path: PathBuf,
    f: impl FnOnce(&mut dc_core::rusqlite::Connection) -> dc_core::Result<T> + Send + 'static,
) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db::open(&db_path)?;
        f(&mut conn)
    })
    .await
    .map_err(e)?
    .map_err(e)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    db_path: String,
    backups_dir: String,
    schema_version: i64,
    workers: usize,
    paused: bool,
    platform: &'static str,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> Res<AppInfo> {
    let schema_version = with_conn!(state, |c| db::schema_version(&c))?;
    let pool = state.pool.lock().map_err(e)?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        db_path: state.db_path.to_string_lossy().into_owned(),
        backups_dir: state.backups_dir.to_string_lossy().into_owned(),
        schema_version,
        workers: pool.as_ref().map(|p| p.size()).unwrap_or(0),
        paused: pool.as_ref().is_some_and(|p| p.is_paused()),
        platform: std::env::consts::OS,
    })
}

#[tauri::command]
pub fn library_stats(state: State<'_, AppState>) -> Res<stats::LibraryStats> {
    with_conn!(state, |c| stats::library_stats(&c))
}

#[tauri::command]
pub fn query_tracks(state: State<'_, AppState>, query: TrackQuery) -> Res<TrackPage> {
    with_conn!(state, |c| dc_core::query::query_tracks(&c, &query))
}

#[tauri::command]
pub fn validate_query(state: State<'_, AppState>, query: String) -> Res<i64> {
    with_conn!(state, |c| dc_core::query::query_tracks(
        &c,
        &TrackQuery {
            query,
            limit: Some(1),
            ..Default::default()
        }
    )
    .map(|p| p.total))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRef {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetail {
    track: library::TrackRow,
    fields: Vec<library::FieldValue>,
    notes: Option<String>,
    analysis: Option<analysis::AnalysisResult>,
    cues: Vec<rekordbox::Cue>,
    beatgrids: Vec<analysis::Beatgrid>,
    playlists: Vec<PlaylistRef>,
    contexts: HashMap<String, String>,
}

#[tauri::command]
pub fn track_detail(state: State<'_, AppState>, id: String) -> Res<TrackDetail> {
    with_conn!(state, |c| {
        let mut stmt = c.prepare(
            "SELECT DISTINCT p.id, p.name FROM playlists p JOIN playlist_tracks pt ON pt.playlist_id = p.id WHERE pt.track_id = ?1 ORDER BY p.name",
        )?;
        let playlists = stmt
            .query_map([&id], |r| {
                Ok(PlaylistRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut stmt = c.prepare(
            "SELECT context, decision FROM decisions WHERE track_id = ?1 AND context != 'global'",
        )?;
        let contexts = stmt
            .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<HashMap<String, String>, _>>()?;
        Ok(TrackDetail {
            track: library::get_track(&c, &id)?,
            fields: library::field_values(&c, &id)?,
            notes: library::track_notes(&c, &id)?,
            analysis: analysis::latest_result(&c, &id)?,
            cues: rekordbox::cues(&c, &id)?,
            beatgrids: analysis::beatgrids(&c, &id)?,
            playlists,
            contexts,
        })
    })
}

#[tauri::command]
pub fn waveform(state: State<'_, AppState>, id: String) -> Res<Option<Vec<u8>>> {
    with_conn!(state, |c| analysis::waveform(&c, &id))
}

#[tauri::command]
pub fn set_decision(
    state: State<'_, AppState>,
    ids: Vec<String>,
    decision: Option<String>,
    context: Option<String>,
) -> Res<()> {
    let context = context.unwrap_or_else(|| "global".into());
    with_conn!(state, |c| library::set_decision(
        &c,
        &ids,
        &context,
        decision.as_deref()
    ))
}

#[tauri::command]
pub fn edit_field(
    state: State<'_, AppState>,
    ids: Vec<String>,
    field: String,
    value: Option<String>,
) -> Res<()> {
    with_conn!(state, |c| library::edit_field(
        &c,
        &ids,
        &field,
        value.as_deref()
    ))
}

/// Drop the user's value so the file/import/analysis value applies again.
#[tauri::command]
pub fn revert_field(state: State<'_, AppState>, ids: Vec<String>, field: String) -> Res<()> {
    with_conn!(state, |c| dc_core::util::atomic(&c, |tx| {
        for id in &ids {
            library::clear_field_source(tx, id, &field, library::SOURCE_USER)?;
            library::refresh_fts(tx, id)?;
        }
        library::audit(
            tx,
            "revert",
            "track",
            None,
            serde_json::json!({"ids": ids, "field": field}),
        )
    }))
}

#[tauri::command]
pub fn set_notes(state: State<'_, AppState>, id: String, notes: Option<String>) -> Res<()> {
    with_conn!(state, |c| library::set_notes(
        &c,
        &id,
        notes.as_deref().filter(|n| !n.trim().is_empty())
    ))
}

#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, ids: Vec<String>, favorite: bool) -> Res<()> {
    with_conn!(state, |c| ids
        .iter()
        .try_for_each(|id| library::set_favorite(&c, id, favorite)))
}

#[tauri::command]
pub async fn import_paths(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
    analyze: bool,
) -> Res<library::ImportReport> {
    let cancel = Arc::new(AtomicBool::new(false));
    *state.import_cancel.lock().map_err(e)? = Some(cancel.clone());
    let roots: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    blocking(state.db_path.clone(), move |conn| {
        let rep = library::import_paths(conn, &roots, Some(cancel), &mut |p| {
            let _ = app.emit("import-progress", p);
        })?;
        if analyze && !rep.added_track_ids.is_empty() {
            jobs::enqueue_analysis(conn, &rep.added_track_ids, false)?;
        }
        Ok(rep)
    })
    .await
}

#[tauri::command]
pub fn cancel_import(state: State<'_, AppState>) -> Res<()> {
    if let Some(c) = state.import_cancel.lock().map_err(e)?.as_ref() {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RbImportResult {
    backup: backup::BackupInfo,
    report: rekordbox::RbImportReport,
}

#[tauri::command]
pub async fn import_rekordbox(state: State<'_, AppState>, path: String) -> Res<RbImportResult> {
    let dir = state.backups_dir.clone();
    blocking(state.db_path.clone(), move |conn| {
        let b = backup::create(conn, &dir, "pre-rekordbox-import")?;
        let report = rekordbox::import_xml(conn, std::path::Path::new(&path))?;
        Ok(RbImportResult { backup: b, report })
    })
    .await
}

/// Export `ids` (in order) or, when absent, everything matching `query`.
#[tauri::command]
pub async fn export_tracks(
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
    query: Option<TrackQuery>,
    format: String,
    path: String,
    name: Option<String>,
) -> Res<usize> {
    blocking(state.db_path.clone(), move |conn| {
        let ids = match ids {
            Some(ids) => ids,
            None => dc_core::query::query_track_ids(conn, &query.unwrap_or_default())?,
        };
        let out = std::path::Path::new(&path);
        match format.as_str() {
            "m3u8" => export::export_m3u8(conn, &ids, out),
            "csv" => export::export_csv(conn, &ids, out),
            "rekordbox" => export::export_rekordbox_xml(
                conn,
                &ids,
                name.as_deref().unwrap_or("Deep Cuts Export"),
                out,
            ),
            other => Err(dc_core::Error::Invalid(format!(
                "unknown export format '{other}'"
            ))),
        }
    })
    .await
}

#[tauri::command]
pub fn list_playlists(state: State<'_, AppState>) -> Res<Vec<library::Playlist>> {
    with_conn!(state, |c| library::list_playlists(&c))
}

#[tauri::command]
pub fn create_playlist(
    state: State<'_, AppState>,
    name: String,
    kind: Option<String>,
    rules: Option<String>,
    track_ids: Option<Vec<String>>,
) -> Res<String> {
    with_conn!(state, |c| {
        let id = library::create_playlist(
            &c,
            &name,
            None,
            kind.as_deref().unwrap_or("manual"),
            rules.as_deref(),
        )?;
        if let Some(ids) = track_ids {
            library::set_playlist_tracks(&c, &id, &ids)?;
        }
        Ok(id)
    })
}

#[tauri::command]
pub fn rename_playlist(state: State<'_, AppState>, id: String, name: String) -> Res<()> {
    with_conn!(state, |c| library::rename_playlist(&c, &id, &name))
}

#[tauri::command]
pub fn delete_playlist(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| library::delete_playlist(&c, &id))
}

#[tauri::command]
pub fn add_to_playlist(state: State<'_, AppState>, id: String, track_ids: Vec<String>) -> Res<()> {
    with_conn!(state, |c| library::add_to_playlist(&c, &id, &track_ids))
}

#[tauri::command]
pub fn remove_from_playlist(
    state: State<'_, AppState>,
    id: String,
    track_ids: Vec<String>,
) -> Res<()> {
    with_conn!(state, |c| library::remove_from_playlist(
        &c, &id, &track_ids
    ))
}

#[tauri::command]
pub fn set_playlist_order(
    state: State<'_, AppState>,
    id: String,
    track_ids: Vec<String>,
) -> Res<()> {
    with_conn!(state, |c| library::set_playlist_tracks(&c, &id, &track_ids))
}

#[tauri::command]
pub fn enqueue_analysis(
    state: State<'_, AppState>,
    ids: Vec<String>,
    force: bool,
) -> Res<jobs::EnqueueReport> {
    with_conn!(state, |c| jobs::enqueue_analysis(&c, &ids, force))
}

#[tauri::command]
pub fn queue_summary(state: State<'_, AppState>) -> Res<jobs::QueueSummary> {
    with_conn!(state, |c| jobs::summary(&c))
}

#[tauri::command]
pub fn recent_jobs(
    state: State<'_, AppState>,
    status: Option<String>,
    limit: Option<i64>,
) -> Res<Vec<jobs::JobRow>> {
    with_conn!(state, |c| jobs::recent(
        &c,
        status.as_deref(),
        limit.unwrap_or(200)
    ))
}

#[tauri::command]
pub fn cancel_queued(state: State<'_, AppState>) -> Res<usize> {
    with_conn!(state, |c| jobs::cancel_queued(&c))
}

#[tauri::command]
pub fn retry_failed(state: State<'_, AppState>) -> Res<usize> {
    with_conn!(state, |c| jobs::retry_failed(&c))
}

#[tauri::command]
pub fn clear_finished(state: State<'_, AppState>) -> Res<usize> {
    with_conn!(state, |c| jobs::clear_finished(&c))
}

#[tauri::command]
pub fn set_paused(state: State<'_, AppState>, paused: bool) -> Res<()> {
    if let Some(p) = state.pool.lock().map_err(e)?.as_ref() {
        p.set_paused(paused);
    }
    Ok(())
}

#[tauri::command]
pub fn set_profile(app: AppHandle, state: State<'_, AppState>, profile: String) -> Res<usize> {
    if !["light", "balanced", "maximum"].contains(&profile.as_str()) {
        return Err(format!("unknown profile '{profile}'"));
    }
    with_conn!(state, |c| library::set_setting(
        &c,
        "analysisProfile",
        &profile
    ))?;
    state.start_pool(&app, &profile);
    Ok(jobs::workers_for_profile(&profile))
}

#[tauri::command]
pub fn create_backup(state: State<'_, AppState>) -> Res<backup::BackupInfo> {
    let dir = state.backups_dir.clone();
    with_conn!(state, |c| backup::create(&c, &dir, "manual"))
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Res<Vec<backup::BackupInfo>> {
    backup::list(&state.backups_dir).map_err(e)
}

/// Restore a snapshot. Workers are stopped for the duration; a safety
/// backup of the current state is taken first.
#[tauri::command]
pub fn restore_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Res<backup::BackupInfo> {
    let path = PathBuf::from(path);
    if path.parent() != Some(state.backups_dir.as_path()) {
        return Err("only backups from the Deep Cuts backup folder can be restored".into());
    }
    state.stop_pool();
    let result = {
        let mut c = state.conn.lock().map_err(e)?;
        backup::restore(&mut c, &path, &state.backups_dir).map_err(e)
    };
    let profile = with_conn!(state, |c| library::get_setting(&c, "analysisProfile"))?
        .unwrap_or_else(|| "balanced".into());
    state.start_pool(&app, &profile);
    result
}

#[tauri::command]
pub async fn check_missing(state: State<'_, AppState>) -> Res<(usize, usize)> {
    blocking(state.db_path.clone(), |c| library::check_missing_files(c)).await
}

#[tauri::command]
pub fn duplicate_groups(state: State<'_, AppState>) -> Res<Vec<library::DuplicateGroup>> {
    with_conn!(state, |c| library::duplicate_groups(&c, 200))
}

#[tauri::command]
pub fn integrity_check(state: State<'_, AppState>) -> Res<Vec<String>> {
    with_conn!(state, |c| db::integrity_check(&c))
}

const SETTINGS: &[(&str, &str)] = &[
    ("language", "en"),
    ("analysisProfile", "balanced"),
    ("autoAnalyze", "true"),
];

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Res<HashMap<String, String>> {
    with_conn!(state, |c| {
        let mut m = HashMap::new();
        for (k, default) in SETTINGS {
            m.insert(
                k.to_string(),
                library::get_setting(&c, k)?.unwrap_or_else(|| default.to_string()),
            );
        }
        Ok(m)
    })
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Res<()> {
    if !SETTINGS.iter().any(|(k, _)| *k == key) {
        return Err(format!("unknown setting '{key}'"));
    }
    with_conn!(state, |c| library::set_setting(&c, &key, &value))
}
