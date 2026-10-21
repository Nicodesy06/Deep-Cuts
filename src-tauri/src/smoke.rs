//! Installer smoke test. When `DEEPCUTS_SMOKE` names a folder of audio
//! files, the installed app imports and analyses them (with the bundled
//! model), the webview plays each one through the `dcaudio` protocol, and the
//! combined report is written to `<folder>/smoke-report.json` before the app
//! exits. CI runs this on the real Windows and macOS installers. Nothing
//! happens without the variable.

use crate::AppState;
use dc_core::{analysis, db, library, ml};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

fn smoke_dir() -> Option<PathBuf> {
    std::env::var_os("DEEPCUTS_SMOKE").map(PathBuf::from)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokeTrack {
    id: String,
    file: String,
    bpm: Option<f64>,
    key: Option<String>,
    model_tags: usize,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokePlan {
    os: &'static str,
    arch: &'static str,
    exe: Option<String>,
    resource_dir: Option<String>,
    model_file: Option<String>,
    model_device: Option<String>,
    import_failures: Vec<String>,
    tracks: Vec<SmokeTrack>,
}

#[tauri::command]
pub async fn smoke_plan(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<SmokePlan>, String> {
    let Some(dir) = smoke_dir() else {
        return Ok(None);
    };
    let db_path = state.db_path.clone();
    let resource_dir = app.path().resource_dir().ok();
    tauri::async_runtime::spawn_blocking(move || -> dc_core::Result<SmokePlan> {
        let mut conn = db::open(&db_path)?;
        let rep = library::import_paths(&mut conn, &[dir], None, &mut |_| {})?;
        let opts = analysis::AnalyzeOptions::from_settings(&conn);
        let mut tracks = Vec::new();
        for id in &rep.added_track_ids {
            let path = library::track_path(&conn, id)?;
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            let res = analysis::analyze_file_with(&path, &|| false, &opts)
                .and_then(|a| analysis::store(&conn, id, &a, 0).map(|_| a));
            let (bpm, key, error) = match res {
                Ok(a) => (
                    a.result.tempo.map(|t| t.bpm),
                    a.result.key.map(|k| k.camelot),
                    None,
                ),
                Err(e) => (None, None, Some(e.to_string())),
            };
            tracks.push(SmokeTrack {
                id: id.clone(),
                file,
                bpm,
                key,
                model_tags: ml::for_track(&conn, id).map(|v| v.len()).unwrap_or(0),
                error,
            });
        }
        tracks.sort_by(|a, b| a.file.cmp(&b.file));
        Ok(SmokePlan {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            exe: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            resource_dir: resource_dir.map(|p| p.to_string_lossy().into_owned()),
            model_file: ml::find_model().map(|p| p.to_string_lossy().into_owned()),
            model_device: ml::shared(&conn).map(|t| t.device.clone()),
            import_failures: rep.failed.iter().map(|f| format!("{f:?}")).collect(),
            tracks,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map(Some)
    .map_err(|e| e.to_string())
}

/// Write the report (plan + webview playback results) and quit.
#[tauri::command]
pub fn smoke_finish(app: AppHandle, report: serde_json::Value) -> Result<(), String> {
    let dir = smoke_dir().ok_or("not in smoke mode")?;
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("smoke-report.json"), text).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}
