//! IPC commands. Thin wrappers over dc-core; errors are returned as strings
//! for display in the UI.

use crate::AppState;
use dc_core::query::{TrackPage, TrackQuery};
use dc_core::{
    analysis, backup, cues, db, export, fixes, grid, history, jobs, library, rekordbox, stats,
    tags, tagwrite,
};
use dc_core::{
    bulk, csvimport, dedupe, djlib, dna, fileops, map, playlist_tools, relocate, sets, similarity,
};
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
    tags: Vec<tags::TrackTag>,
    history: Vec<history::ChangeRow>,
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
            tags: tags::for_track(&c, &id)?,
            history: history::track_history(&c, &id, 50)?,
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
    with_conn!(state, |c| library::revert_field(&c, &ids, &field))
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
    with_conn!(state, |c| library::set_favorite(&c, &ids, favorite))
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
        dc_core::recipes::run_on_import(conn, &rep.added_track_ids)?;
        Ok(rep)
    })
    .await
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AutotagProgress {
    done: usize,
    total: usize,
    current: String,
}

/// Look the tracks up online (only when the user starts it). Returns the
/// proposals; nothing is written.
#[tauri::command]
pub async fn autotag_run(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    options: dc_core::autotag::Options,
) -> Res<Vec<dc_core::autotag::TrackResult>> {
    let cancel = Arc::new(AtomicBool::new(false));
    *state.autotag_cancel.lock().map_err(e)? = Some(cancel.clone());
    blocking(state.db_path.clone(), move |conn| {
        let http = dc_core::autotag::Http::new(options.discogs_token.clone());
        let mut out = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let r = dc_core::autotag::tag_track(conn, &http, id, &options)?;
            let _ = app.emit(
                "autotag-progress",
                AutotagProgress {
                    done: i + 1,
                    total: ids.len(),
                    current: format!("{} – {}", r.artist, r.title),
                },
            );
            out.push(r);
        }
        Ok(out)
    })
    .await
}

#[tauri::command]
pub fn autotag_cancel(state: State<'_, AppState>) -> Res<()> {
    if let Some(c) = state.autotag_cancel.lock().map_err(e)?.as_ref() {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutotagApplied {
    fields: usize,
    artwork: tagwrite::WriteReport,
}

#[tauri::command]
pub async fn autotag_apply(
    state: State<'_, AppState>,
    proposals: Vec<dc_core::autotag::SourcedProposal>,
    keep_backup: bool,
) -> Res<AutotagApplied> {
    let dir = state.backups_dir.join("audio-originals");
    blocking(state.db_path.clone(), move |conn| {
        let http = dc_core::autotag::Http::new(None);
        let (fields, artwork) = dc_core::autotag::apply(
            conn,
            &http,
            &proposals,
            keep_backup.then_some(dir.as_path()),
        )?;
        Ok(AutotagApplied { fields, artwork })
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

/// Import another DJ application's library (backup first).
#[tauri::command]
pub async fn import_dj(
    state: State<'_, AppState>,
    app: String,
    path: String,
) -> Res<RbImportResult> {
    let dir = state.backups_dir.clone();
    blocking(state.db_path.clone(), move |conn| {
        let b = backup::create(conn, &dir, &format!("pre-{app}-import"))?;
        let report = djlib::import_from(conn, &app, std::path::Path::new(&path))?;
        Ok(RbImportResult { backup: b, report })
    })
    .await
}

/// Export to another DJ application. `whole` = every track and playlist;
/// otherwise `ids` (or the tracks matching `query`) as one playlist.
#[tauri::command]
pub async fn export_dj(
    state: State<'_, AppState>,
    app: String,
    whole: bool,
    ids: Option<Vec<String>>,
    query: Option<TrackQuery>,
    name: Option<String>,
    target: String,
) -> Res<djlib::ExportSummary> {
    blocking(state.db_path.clone(), move |conn| {
        let lib = if whole {
            djlib::whole_library(conn)?
        } else {
            let ids = match ids {
                Some(ids) => ids,
                None => dc_core::query::query_track_ids(conn, &query.unwrap_or_default())?,
            };
            djlib::for_ids(conn, &ids, name.as_deref().unwrap_or("Deep Cuts Export"))?
        };
        djlib::export_to(&app, &lib, std::path::Path::new(&target))
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
        let playlist = query.as_ref().and_then(|q| q.playlist_id.clone());
        let ids = match ids {
            Some(ids) => ids,
            None => dc_core::query::query_track_ids(conn, &query.unwrap_or_default())?,
        };
        let out = std::path::Path::new(&path);
        match format.as_str() {
            "html" | "pdf" => dc_core::printable::export(
                conn,
                &ids,
                name.as_deref().unwrap_or("Deep Cuts"),
                playlist.as_deref(),
                &format,
                out,
            ),
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
    with_conn!(state, |c| library::reorder_playlist(
        &c,
        &id,
        &track_ids,
        "playlist-reorder"
    ))
}

#[tauri::command]
pub fn enqueue_analysis(
    state: State<'_, AppState>,
    ids: Vec<String>,
    force: bool,
) -> Res<jobs::EnqueueReport> {
    with_conn!(state, |c| jobs::enqueue_analysis(&c, &ids, force))
}

/// Re-queue every track (e.g. after changing the BPM range).
#[tauri::command]
pub fn reanalyze_all(state: State<'_, AppState>) -> Res<jobs::EnqueueReport> {
    with_conn!(state, |c| {
        let ids: Vec<String> = {
            let mut stmt = c.prepare("SELECT id FROM live_tracks")?;
            stmt.query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        jobs::enqueue_analysis(&c, &ids, true)
    })
}

#[tauri::command]
pub fn compare_sources(state: State<'_, AppState>) -> Res<Vec<dc_core::compare::SourceCount>> {
    with_conn!(state, |c| dc_core::compare::sources(&c))
}

#[tauri::command]
pub async fn compare_analysis(
    state: State<'_, AppState>,
    source: String,
) -> Res<dc_core::compare::CompareReport> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::compare::compare(c, &source, 500)
    })
    .await
}

#[tauri::command]
pub fn list_cue_templates(state: State<'_, AppState>) -> Res<Vec<dc_core::cues::Template>> {
    with_conn!(state, |c| dc_core::cues::list_templates(&c))
}

#[tauri::command]
pub fn save_cue_template(
    state: State<'_, AppState>,
    template: dc_core::cues::Template,
) -> Res<dc_core::cues::Template> {
    with_conn!(state, |c| dc_core::cues::save_template(&c, template))
}

#[tauri::command]
pub fn delete_cue_template(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| dc_core::cues::delete_template(&c, &id))
}

#[tauri::command]
pub async fn apply_cue_template(
    state: State<'_, AppState>,
    template: dc_core::cues::Template,
    ids: Vec<String>,
) -> Res<dc_core::cues::TemplateReport> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::cues::apply_template(c, &template, &ids)
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VdjFilter {
    filter: Option<String>,
    unsupported: Vec<String>,
}

/// A smart-crate rule in VirtualDJ filter syntax.
#[tauri::command]
pub fn virtualdj_filter(query: String) -> Res<VdjFilter> {
    match dc_core::rules::to_virtualdj(&query).map_err(|e| e.to_string())? {
        Ok(f) => Ok(VdjFilter {
            filter: Some(f),
            unsupported: vec![],
        }),
        Err(bad) => Ok(VdjFilter {
            filter: None,
            unsupported: bad,
        }),
    }
}

#[tauri::command]
pub async fn track_character(
    state: State<'_, AppState>,
    id: String,
) -> Res<dc_core::dna::Character> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::dna::character(c, &id)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    file: Option<String>,
    enabled: bool,
    device: Option<String>,
    error: Option<String>,
    /// Analysed tracks still without results of the current model.
    missing: usize,
}

/// Whether the audio model is present, enabled and on which device.
#[tauri::command]
pub async fn model_status(state: State<'_, AppState>) -> Res<ModelStatus> {
    blocking(state.db_path.clone(), move |c| {
        let enabled = library::get_setting(c, "aiModel")?.as_deref() != Some("off");
        let file = dc_core::ml::find_model().map(|p| p.to_string_lossy().into_owned());
        let (device, error) = if enabled && file.is_some() {
            match dc_core::ml::shared(c) {
                Some(t) => (Some(t.device.clone()), None),
                None => (None, Some("the model could not be loaded".to_string())),
            }
        } else {
            (None, None)
        };
        let missing = if device.is_some() {
            dc_core::ml::tracks_without_model(c)?.len()
        } else {
            0
        };
        Ok(ModelStatus {
            file,
            enabled,
            device,
            error,
            missing,
        })
    })
    .await
}

/// Queue re-analysis of the tracks without current model results.
#[tauri::command]
pub fn model_reanalyze(state: State<'_, AppState>) -> Res<usize> {
    with_conn!(state, |c| {
        let ids = dc_core::ml::tracks_without_model(&c)?;
        jobs::enqueue_analysis(&c, &ids, true).map(|_| ids.len())
    })
}

#[tauri::command]
pub async fn model_suggest_tags(state: State<'_, AppState>) -> Res<usize> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::ml::suggest_tags(c, None)
    })
    .await
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
pub fn find_replace_preview(
    state: State<'_, AppState>,
    ids: Vec<String>,
    spec: bulk::FindReplace,
) -> Res<Vec<bulk::Proposal>> {
    with_conn!(state, |c| bulk::preview(&c, &ids, &spec))
}

#[tauri::command]
pub fn find_replace_apply(
    state: State<'_, AppState>,
    proposals: Vec<bulk::Proposal>,
) -> Res<usize> {
    with_conn!(state, |c| bulk::apply(&c, &proposals))
}

#[tauri::command]
pub fn combine_playlists(
    state: State<'_, AppState>,
    name: String,
    ids: Vec<String>,
    op: playlist_tools::Combine,
) -> Res<String> {
    with_conn!(state, |c| playlist_tools::combine(&c, &name, &ids, op))
}

#[tauri::command]
pub fn shuffle_playlist(state: State<'_, AppState>, id: String) -> Res<()> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    with_conn!(state, |c| playlist_tools::shuffle(&c, &id, seed))
}

#[tauri::command]
pub fn prefix_playlists(
    state: State<'_, AppState>,
    ids: Vec<String>,
    prefix: String,
) -> Res<usize> {
    with_conn!(state, |c| playlist_tools::add_prefix(&c, &ids, &prefix))
}

#[tauri::command]
pub fn track_playlists(
    state: State<'_, AppState>,
    id: String,
) -> Res<Vec<playlist_tools::Occurrence>> {
    with_conn!(state, |c| playlist_tools::occurrences(&c, &id))
}

#[tauri::command]
pub async fn match_playlist_file(
    state: State<'_, AppState>,
    path: String,
) -> Res<dc_core::playlist_match::MatchReport> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::playlist_match::read_and_match(c, std::path::Path::new(&path))
    })
    .await
}

/// Import an M3U/M3U8 file as a playlist (missing files imported first).
#[tauri::command]
pub async fn import_m3u(
    state: State<'_, AppState>,
    path: String,
) -> Res<dc_core::m3uimport::M3uImport> {
    let r = blocking(state.db_path.clone(), move |c| {
        dc_core::m3uimport::import(c, std::path::Path::new(&path))
    })
    .await?;
    if !r.new_track_ids.is_empty() {
        with_conn!(state, |c| jobs::enqueue_analysis(
            &c,
            &r.new_track_ids,
            false
        ))?;
    }
    Ok(r)
}

/// Read Rekordbox My Tags from the comments of every track.
#[tauri::command]
pub fn mytag_scan(state: State<'_, AppState>) -> Res<dc_core::mytag::MyTagReport> {
    with_conn!(state, |c| dc_core::mytag::scan_library(&c))
}

/// One M3U8 per My Tag into a folder; returns the number of files.
#[tauri::command]
pub fn mytag_export_m3u8(state: State<'_, AppState>, dir: String) -> Res<usize> {
    with_conn!(state, |c| dc_core::mytag::export_m3u8(
        &c,
        std::path::Path::new(&dir)
    )
    .map(|v| v.len()))
}

#[tauri::command]
pub fn create_matched_playlist(
    state: State<'_, AppState>,
    name: String,
    track_ids: Vec<String>,
) -> Res<String> {
    with_conn!(state, |c| dc_core::playlist_match::create_playlist(
        &c, &name, &track_ids
    ))
}

#[tauri::command]
pub fn csv_read(path: String) -> Res<csvimport::Sheet> {
    csvimport::read(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn csv_preview(
    state: State<'_, AppState>,
    path: String,
    mapping: csvimport::Mapping,
) -> Res<csvimport::ImportPreview> {
    blocking(state.db_path.clone(), move |c| {
        let sheet = csvimport::read(std::path::Path::new(&path))?;
        csvimport::preview(c, &sheet, &mapping)
    })
    .await
}

#[tauri::command]
pub fn csv_apply(state: State<'_, AppState>, proposals: Vec<bulk::Proposal>) -> Res<usize> {
    with_conn!(state, |c| bulk::apply_as(&c, "csv-import", &proposals))
}

#[tauri::command]
pub fn relocate_preview(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Res<relocate::RelocatePreview> {
    with_conn!(state, |c| relocate::preview(&c, &from, &to))
}

#[tauri::command]
pub fn relocate_apply(
    state: State<'_, AppState>,
    from: String,
    to: String,
    only_found: bool,
) -> Res<usize> {
    with_conn!(state, |c| relocate::apply(&c, &from, &to, only_found))
}

#[tauri::command]
pub fn missing_folders(state: State<'_, AppState>) -> Res<Vec<(String, i64)>> {
    with_conn!(state, |c| relocate::missing_folders(&c, 10))
}

#[tauri::command]
pub async fn dna_genre_suggestions(
    state: State<'_, AppState>,
    min_share: f32,
) -> Res<dna::LabelReport> {
    blocking(state.db_path.clone(), move |c| {
        dna::genre_suggestions(c, min_share)
    })
    .await
}

/// Compute tag suggestions and put them in the review queue.
#[tauri::command]
pub async fn dna_tag_suggestions(
    state: State<'_, AppState>,
    min_share: f32,
) -> Res<dna::LabelReport> {
    blocking(state.db_path.clone(), move |c| {
        let rep = dna::tag_suggestions(c, min_share, 5)?;
        dna::store_tag_suggestions(c, &rep)?;
        Ok(rep)
    })
    .await
}

#[tauri::command]
pub async fn dna_groups(
    state: State<'_, AppState>,
    k: usize,
    scope: String,
) -> Res<Vec<dna::Group>> {
    blocking(state.db_path.clone(), move |c| {
        dna::auto_groups(c, k, &scope)
    })
    .await
}

#[tauri::command]
pub async fn dna_update_energy(state: State<'_, AppState>) -> Res<usize> {
    blocking(state.db_path.clone(), |c| dna::update_energy(c)).await
}

/// (track id, tag id, tag name, confidence)
type PendingSuggestion = (String, String, String, Option<f64>);

/// Tag suggestions waiting for review (all tracks).
#[tauri::command]
pub fn pending_tag_suggestions(state: State<'_, AppState>) -> Res<Vec<PendingSuggestion>> {
    with_conn!(state, |c| {
        let mut stmt = c.prepare(
            "SELECT tt.track_id, tt.tag_id, t.name, tt.confidence FROM track_tags tt JOIN tags t ON t.id = tt.tag_id
             JOIN tracks tr ON tr.id = tt.track_id
             WHERE tt.origin = 'ai' AND tt.status = 'suggested' AND tr.archived = 0 ORDER BY t.name, tt.confidence DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok::<_, dc_core::Error>(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })
}

#[tauri::command]
pub fn tracks_by_ids(state: State<'_, AppState>, ids: Vec<String>) -> Res<Vec<library::TrackRow>> {
    with_conn!(state, |c| dc_core::query::rows_for_ids(&c, &ids))
}

#[tauri::command]
pub fn dna_apply_genres(state: State<'_, AppState>, proposals: Vec<bulk::Proposal>) -> Res<usize> {
    with_conn!(state, |c| bulk::apply_as(
        &c,
        "genre-suggestions",
        &proposals
    ))
}

/// Build a set from a seed track (graph walk) and save it as a new set.
#[tauri::command]
pub async fn build_set_from_seed(
    state: State<'_, AppState>,
    seed: String,
    len: usize,
    scope: String,
    mode: String,
    name: String,
) -> Res<String> {
    blocking(state.db_path.clone(), move |c| {
        let path = sets::build_path(c, &seed, len, &scope, &mode)?;
        let id = library::create_playlist(c, &name, None, "set", None)?;
        library::set_playlist_tracks(c, &id, &path)?;
        Ok(id)
    })
    .await
}

#[tauri::command]
pub fn list_recipes(state: State<'_, AppState>) -> Res<Vec<dc_core::recipes::Recipe>> {
    with_conn!(state, |c| dc_core::recipes::list(&c))
}

#[tauri::command]
pub fn save_recipe(state: State<'_, AppState>, recipe: dc_core::recipes::Recipe) -> Res<String> {
    with_conn!(state, |c| dc_core::recipes::save(&c, &recipe))
}

#[tauri::command]
pub fn delete_recipe(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| dc_core::recipes::delete(&c, &id))
}

#[tauri::command]
pub fn recipe_count(state: State<'_, AppState>, recipe: dc_core::recipes::Recipe) -> Res<usize> {
    with_conn!(state, |c| dc_core::recipes::matching(&c, &recipe, None)
        .map(|v| v.len()))
}

#[tauri::command]
pub async fn run_recipe(
    state: State<'_, AppState>,
    id: String,
) -> Res<dc_core::recipes::RunReport> {
    blocking(state.db_path.clone(), move |c| {
        dc_core::recipes::run(c, &id, None)
    })
    .await
}

#[tauri::command]
pub fn suggest_keep(state: State<'_, AppState>, ids: Vec<String>) -> Res<Option<String>> {
    with_conn!(state, |c| dedupe::suggest_keep(&c, &ids))
}

#[tauri::command]
pub async fn merge_duplicates(
    state: State<'_, AppState>,
    keep: String,
    others: Vec<String>,
) -> Res<dedupe::MergeReport> {
    let trash = state
        .db_path
        .parent()
        .map(|p| p.join("trash"))
        .unwrap_or_else(dc_core::paths::trash_dir);
    blocking(state.db_path.clone(), move |c| {
        dedupe::merge(c, &keep, &others, &trash)
    })
    .await
}

#[tauri::command]
pub async fn unused_files(state: State<'_, AppState>, root: String) -> Res<Vec<String>> {
    blocking(state.db_path.clone(), move |c| {
        dedupe::unused_files(c, std::path::Path::new(&root))
    })
    .await
}

#[tauri::command]
pub fn broken_playlists(state: State<'_, AppState>) -> Res<Vec<dedupe::BrokenPlaylist>> {
    with_conn!(state, |c| dedupe::broken_playlists(&c))
}

#[tauri::command]
pub fn set_archived(state: State<'_, AppState>, ids: Vec<String>, archived: bool) -> Res<()> {
    with_conn!(state, |c| library::set_archived(&c, &ids, archived))
}

#[tauri::command]
pub fn transcode_suspects(state: State<'_, AppState>) -> Res<Vec<library::TranscodeSuspect>> {
    with_conn!(state, |c| library::transcode_suspects(&c, 500))
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

// ---------------------------------------------------------------------------
// Undo / redo
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> Res<Option<history::BatchInfo>> {
    with_conn!(state, |c| history::undo(&c))
}

#[tauri::command]
pub fn redo(state: State<'_, AppState>) -> Res<Option<history::BatchInfo>> {
    with_conn!(state, |c| history::redo(&c))
}

#[tauri::command]
pub fn recent_changes(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Res<Vec<history::BatchInfo>> {
    with_conn!(state, |c| history::recent(&c, limit.unwrap_or(50)))
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Res<Vec<tags::Tag>> {
    with_conn!(state, |c| tags::list(&c))
}

#[tauri::command]
pub fn create_tag(
    state: State<'_, AppState>,
    name: String,
    category: String,
    parent_id: Option<String>,
) -> Res<String> {
    with_conn!(state, |c| tags::create(
        &c,
        &name,
        &category,
        parent_id.as_deref()
    ))
}

#[tauri::command]
pub fn update_tag(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    category: Option<String>,
    set_parent: Option<bool>,
    parent_id: Option<String>,
) -> Res<()> {
    with_conn!(state, |c| dc_core::util::atomic(&c, |tx| {
        if let Some(n) = &name {
            tags::rename(tx, &id, n)?;
        }
        if let Some(cat) = &category {
            tags::set_category(tx, &id, cat)?;
        }
        if set_parent == Some(true) {
            tags::set_parent(tx, &id, parent_id.as_deref())?;
        }
        Ok(())
    }))
}

#[tauri::command]
pub fn delete_tag(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| tags::delete(&c, &id))
}

#[tauri::command]
pub fn add_tag_synonym(state: State<'_, AppState>, id: String, synonym: String) -> Res<()> {
    with_conn!(state, |c| tags::add_synonym(&c, &id, &synonym))
}

#[tauri::command]
pub fn remove_tag_synonym(state: State<'_, AppState>, synonym: String) -> Res<()> {
    with_conn!(state, |c| tags::remove_synonym(&c, &synonym))
}

#[tauri::command]
pub fn set_quick_tag(state: State<'_, AppState>, slot: i64, tag_id: Option<String>) -> Res<()> {
    with_conn!(state, |c| tags::set_quick_slot(&c, slot, tag_id.as_deref()))
}

/// Assign (on=true), remove (on=false) or toggle (on=None) a tag.
#[tauri::command]
pub fn tag_tracks(
    state: State<'_, AppState>,
    ids: Vec<String>,
    tag_id: String,
    on: Option<bool>,
) -> Res<bool> {
    with_conn!(state, |c| match on {
        Some(on) => tags::assign(&c, &ids, &tag_id, on).map(|_| on),
        None => tags::toggle(&c, &ids, &tag_id),
    })
}

/// Quick Tag: toggle the tag in `slot` on the tracks. Returns the tag name.
#[tauri::command]
pub fn quick_tag(
    state: State<'_, AppState>,
    ids: Vec<String>,
    slot: i64,
) -> Res<Option<(String, bool)>> {
    with_conn!(state, |c| {
        let Some(tag) = tags::quick_slot_tag(&c, slot)? else {
            return Ok(None);
        };
        let on = tags::toggle(&c, &ids, &tag)?;
        let name: String =
            c.query_row("SELECT name FROM tags WHERE id = ?1", [&tag], |r| r.get(0))?;
        Ok(Some((name, on)))
    })
}

#[tauri::command]
pub fn review_tag_suggestion(
    state: State<'_, AppState>,
    track_id: String,
    tag_id: String,
    accept: bool,
) -> Res<()> {
    with_conn!(state, |c| tags::review_suggestion(
        &c, &track_id, &tag_id, accept
    ))
}

// ---------------------------------------------------------------------------
// Smart fixes
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn fix_rules() -> Vec<fixes::RuleInfo> {
    fixes::RULES.to_vec()
}

#[tauri::command]
pub async fn preview_fixes(
    state: State<'_, AppState>,
    query: TrackQuery,
    rules: Vec<String>,
    limit: Option<usize>,
) -> Res<Vec<fixes::Proposal>> {
    blocking(state.db_path.clone(), move |c| {
        let ids = dc_core::query::query_track_ids(c, &query)?;
        fixes::preview(c, &ids, &rules, limit.unwrap_or(2000))
    })
    .await
}

#[tauri::command]
pub fn apply_fixes(state: State<'_, AppState>, fixes: Vec<fixes::AcceptedFix>) -> Res<usize> {
    with_conn!(state, |c| fixes::apply(&c, &fixes))
}

// ---------------------------------------------------------------------------
// Cues
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn grid_set(
    state: State<'_, AppState>,
    track_id: String,
    bpm: f64,
    first_beat_ms: f64,
) -> Res<()> {
    with_conn!(state, |c| grid::set_user(
        &c,
        &track_id,
        grid::Grid { bpm, first_beat_ms }
    ))
}

#[tauri::command]
pub fn grid_clear(state: State<'_, AppState>, track_id: String) -> Res<()> {
    with_conn!(state, |c| grid::clear_user(&c, &track_id))
}

#[tauri::command]
pub fn grid_loop(state: State<'_, AppState>, track_id: String, ms: f64, beats: f64) -> Res<String> {
    with_conn!(state, |c| grid::add_loop(&c, &track_id, ms, beats))
}

/// Audio envelope (full band, low band) of a window of the track.
#[tauri::command]
pub async fn grid_window(
    state: State<'_, AppState>,
    track_id: String,
    start_ms: f64,
    len_ms: f64,
    points: usize,
) -> Res<Vec<(f32, f32)>> {
    blocking(state.db_path.clone(), move |c| {
        let path = library::track_path(c, &track_id)?;
        grid::envelope_window(&path, start_ms, len_ms, points)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn add_cue(
    state: State<'_, AppState>,
    track_id: String,
    kind: String,
    slot: Option<i64>,
    start_ms: f64,
    end_ms: Option<f64>,
    name: Option<String>,
    color: Option<String>,
) -> Res<String> {
    with_conn!(state, |c| cues::add(
        &c,
        &track_id,
        &cues::NewCue {
            kind: &kind,
            slot,
            start_ms,
            end_ms,
            name: name.as_deref(),
            color: color.as_deref()
        }
    ))
}

#[tauri::command]
pub fn accept_cue(state: State<'_, AppState>, id: String, hot_slot: Option<i64>) -> Res<String> {
    with_conn!(state, |c| cues::accept(&c, &id, hot_slot))
}

#[tauri::command]
pub fn delete_cue(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| cues::delete(&c, &id))
}

#[tauri::command]
pub fn rename_cue(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    color: Option<String>,
) -> Res<()> {
    with_conn!(state, |c| cues::rename(
        &c,
        &id,
        name.as_deref(),
        color.as_deref()
    ))
}

// ---------------------------------------------------------------------------
// Writing tags to files
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn plan_tag_write(
    state: State<'_, AppState>,
    ids: Vec<String>,
    fields: Option<Vec<String>>,
) -> Res<Vec<tagwrite::WritePlan>> {
    blocking(state.db_path.clone(), move |c| {
        let opts = tagwrite::WriteOptions::from_settings(c, fields);
        tagwrite::plan_with(c, &ids, &opts)
    })
    .await
}

#[tauri::command]
pub async fn write_tags(
    state: State<'_, AppState>,
    ids: Vec<String>,
    keep_backup: bool,
    fields: Option<Vec<String>>,
) -> Res<tagwrite::WriteReport> {
    let dir = state.backups_dir.join("audio-originals");
    blocking(state.db_path.clone(), move |c| {
        let opts = tagwrite::WriteOptions::from_settings(c, fields);
        tagwrite::write_with(c, &ids, keep_backup.then_some(dir.as_path()), &opts)
    })
    .await
}

#[tauri::command]
pub async fn set_cover(
    state: State<'_, AppState>,
    ids: Vec<String>,
    image: String,
    keep_backup: bool,
) -> Res<tagwrite::WriteReport> {
    let dir = state.backups_dir.join("audio-originals");
    blocking(state.db_path.clone(), move |c| {
        dc_core::artwork::set_cover(
            c,
            &ids,
            std::path::Path::new(&image),
            keep_backup.then_some(dir.as_path()),
        )
    })
    .await
}

#[tauri::command]
pub async fn remove_covers(
    state: State<'_, AppState>,
    ids: Vec<String>,
    keep_backup: bool,
) -> Res<tagwrite::WriteReport> {
    let dir = state.backups_dir.join("audio-originals");
    blocking(state.db_path.clone(), move |c| {
        dc_core::artwork::remove_covers(c, &ids, keep_backup.then_some(dir.as_path()))
    })
    .await
}

// ---------------------------------------------------------------------------
// Renaming / moving files
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn plan_rename(
    state: State<'_, AppState>,
    ids: Vec<String>,
    pattern: String,
    target_dir: Option<String>,
) -> Res<Vec<fileops::MovePlan>> {
    blocking(state.db_path.clone(), move |c| {
        fileops::plan(
            c,
            &ids,
            &pattern,
            target_dir.as_deref().map(std::path::Path::new),
        )
    })
    .await
}

#[tauri::command]
pub async fn apply_rename(
    state: State<'_, AppState>,
    plans: Vec<fileops::MovePlan>,
) -> Res<fileops::ApplyReport> {
    blocking(state.db_path.clone(), move |c| fileops::apply(c, &plans)).await
}

// ---------------------------------------------------------------------------
// Watch folders
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_watch_folders(state: State<'_, AppState>) -> Res<Vec<library::WatchFolder>> {
    with_conn!(state, |c| library::watch_folders(&c))
}

#[tauri::command]
pub fn add_watch_folder(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<String> {
    let key = with_conn!(state, |c| library::add_watch_folder(
        &c,
        std::path::Path::new(&path)
    ))?;
    crate::watch::restart(&app);
    crate::watch::scan_now(&app, vec![PathBuf::from(&key)]);
    Ok(key)
}

#[tauri::command]
pub fn remove_watch_folder(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<()> {
    with_conn!(state, |c| library::remove_watch_folder(&c, &path))?;
    crate::watch::restart(&app);
    Ok(())
}

// ---------------------------------------------------------------------------
// Discover: similarity + map
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn similar_tracks(
    state: State<'_, AppState>,
    id: String,
    weights: Option<similarity::Weights>,
    scope: Option<String>,
    limit: Option<usize>,
) -> Res<Vec<similarity::Match>> {
    blocking(state.db_path.clone(), move |c| {
        similarity::similar(
            c,
            &id,
            &weights.unwrap_or_default(),
            scope.as_deref().unwrap_or(""),
            limit.unwrap_or(50),
        )
    })
    .await
}

#[tauri::command]
pub async fn discovery_map(
    state: State<'_, AppState>,
    scope: Option<String>,
) -> Res<map::DiscoveryMap> {
    blocking(state.db_path.clone(), move |c| {
        map::build(c, scope.as_deref().unwrap_or(""))
    })
    .await
}

// ---------------------------------------------------------------------------
// Set Builder
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn set_report(
    state: State<'_, AppState>,
    id: String,
    mode: Option<String>,
) -> Res<sets::SetReport> {
    blocking(state.db_path.clone(), move |c| {
        sets::report(c, &id, mode.as_deref().unwrap_or("smooth"))
    })
    .await
}

#[tauri::command]
pub fn set_chapter(
    state: State<'_, AppState>,
    playlist: String,
    position: i64,
    role: String,
    name: Option<String>,
) -> Res<String> {
    with_conn!(state, |c| sets::set_chapter(
        &c,
        &playlist,
        position,
        &role,
        name.as_deref()
    ))
}

#[tauri::command]
pub fn remove_chapter(state: State<'_, AppState>, id: String) -> Res<()> {
    with_conn!(state, |c| sets::remove_chapter(&c, &id))
}

#[tauri::command]
pub fn set_track_lock(
    state: State<'_, AppState>,
    playlist: String,
    track: String,
    locked: bool,
) -> Res<()> {
    with_conn!(state, |c| sets::set_lock(&c, &playlist, &track, locked))
}

#[tauri::command]
pub fn set_playlist_notes(
    state: State<'_, AppState>,
    playlist: String,
    notes: Option<String>,
) -> Res<()> {
    with_conn!(state, |c| sets::set_notes(
        &c,
        &playlist,
        notes.as_deref().filter(|n| !n.trim().is_empty())
    ))
}

#[tauri::command]
pub async fn magic_sort(
    state: State<'_, AppState>,
    playlist: String,
    mode: String,
) -> Res<sets::SortResult> {
    blocking(state.db_path.clone(), move |c| {
        sets::magic_sort(c, &playlist, &mode)
    })
    .await
}

#[tauri::command]
pub fn apply_set_order(
    state: State<'_, AppState>,
    playlist: String,
    order: Vec<String>,
) -> Res<()> {
    with_conn!(state, |c| sets::apply_order(&c, &playlist, &order))
}

#[tauri::command]
pub async fn suggest_next(
    state: State<'_, AppState>,
    playlist: String,
    after: Option<usize>,
    scope: Option<String>,
    mode: Option<String>,
    limit: Option<usize>,
) -> Res<Vec<sets::Candidate>> {
    blocking(state.db_path.clone(), move |c| {
        sets::suggest_next(
            c,
            &playlist,
            after,
            scope.as_deref().unwrap_or(""),
            mode.as_deref().unwrap_or("smooth"),
            limit.unwrap_or(20),
        )
    })
    .await
}
