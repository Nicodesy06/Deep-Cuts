//! Persistent background job queue and analysis workers.
//!
//! Jobs live in SQLite, so a crash or quit never loses the queue: jobs that
//! were `running` are re-queued when the database is next opened.

use crate::analysis;
use crate::error::{Error, Result};
use crate::util::now_ms;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub const KIND_ANALYZE: &str = "analyze";
const MAX_ATTEMPTS: i64 = 2;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueReport {
    pub queued: usize,
    pub skipped_up_to_date: usize,
    pub already_queued: usize,
}

/// Queue analysis for tracks. Unless `force`, tracks already analysed by the
/// current analyser version on the same file content are skipped.
pub fn enqueue_analysis(
    conn: &Connection,
    track_ids: &[String],
    force: bool,
) -> Result<EnqueueReport> {
    let mut rep = EnqueueReport::default();
    crate::util::atomic(conn, |tx| {
        for id in track_ids {
            let pending: Option<i64> = tx
            .query_row(
                "SELECT id FROM jobs WHERE track_id = ?1 AND kind = ?2 AND status IN ('queued','running') LIMIT 1",
                params![id, KIND_ANALYZE],
                |r| r.get(0),
            )
            .optional()?;
            if pending.is_some() {
                rep.already_queued += 1;
                continue;
            }
            if !force && analysis::is_up_to_date(tx, id)? {
                rep.skipped_up_to_date += 1;
                continue;
            }
            tx.execute(
            "INSERT INTO jobs (kind, track_id, status, created_at, updated_at) VALUES (?1, ?2, 'queued', ?3, ?3)",
            params![KIND_ANALYZE, id, now_ms()],
        )?;
            tx.execute(
                "UPDATE tracks SET analysis_status = 'queued' WHERE id = ?1",
                [id],
            )?;
            rep.queued += 1;
        }
        Ok(())
    })?;
    Ok(rep)
}

pub struct Job {
    pub id: i64,
    pub kind: String,
    pub track_id: Option<String>,
}

/// Identifies this process as the owner of the jobs it claims.
fn process_owner() -> &'static str {
    static OWNER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    OWNER.get_or_init(crate::util::new_id)
}

/// Re-queue jobs left `running` by a process that is gone (crash, kill,
/// power loss). Call once at startup, before starting workers.
pub fn recover_interrupted(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE jobs SET status = 'queued', updated_at = ?1 WHERE status = 'running' AND (owner IS NULL OR owner != ?2)",
        params![now_ms(), process_owner()],
    )?)
}

pub fn claim_next(conn: &Connection) -> Result<Option<Job>> {
    Ok(conn
        .query_row(
            "UPDATE jobs SET status = 'running', attempts = attempts + 1, updated_at = ?1, owner = ?2
             WHERE id = (SELECT id FROM jobs WHERE status = 'queued' ORDER BY priority DESC, id LIMIT 1)
             RETURNING id, kind, track_id",
            params![now_ms(), process_owner()],
            |r| Ok(Job { id: r.get(0)?, kind: r.get(1)?, track_id: r.get(2)? }),
        )
        .optional()?)
}

fn finish(conn: &Connection, job: &Job, outcome: &std::result::Result<(), Error>) -> Result<()> {
    match outcome {
        Ok(()) => {
            conn.execute(
                "UPDATE jobs SET status = 'done', error = NULL, updated_at = ?1 WHERE id = ?2",
                params![now_ms(), job.id],
            )?;
        }
        Err(Error::Cancelled) => {
            conn.execute("UPDATE jobs SET status = 'queued', attempts = attempts - 1, updated_at = ?1 WHERE id = ?2", params![now_ms(), job.id])?;
        }
        Err(e) => {
            let attempts: i64 =
                conn.query_row("SELECT attempts FROM jobs WHERE id = ?1", [job.id], |r| {
                    r.get(0)
                })?;
            // Decoding errors are deterministic; retry only other failures.
            let retry =
                attempts < MAX_ATTEMPTS && !matches!(e, Error::Decode(_) | Error::NotFound(_));
            let status = if retry { "queued" } else { "failed" };
            conn.execute(
                "UPDATE jobs SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                params![status, e.to_string(), now_ms(), job.id],
            )?;
            if !retry && let Some(t) = &job.track_id {
                conn.execute(
                    "UPDATE tracks SET analysis_status = 'failed' WHERE id = ?1",
                    [t],
                )?;
            }
        }
    }
    Ok(())
}

/// (job id, track id, outcome) of an executed job.
pub type JobOutcome = (i64, Option<String>, std::result::Result<(), String>);

/// Execute one job. Returns Ok(None) when the queue is empty.
pub fn run_one(conn: &Connection, cancel: &dyn Fn() -> bool) -> Result<Option<JobOutcome>> {
    let Some(job) = claim_next(conn)? else {
        return Ok(None);
    };
    let outcome = match job.kind.as_str() {
        KIND_ANALYZE => run_analysis(conn, job.track_id.as_deref(), cancel),
        other => Err(Error::Invalid(format!("unknown job kind '{other}'"))),
    };
    finish(conn, &job, &outcome)?;
    Ok(Some((
        job.id,
        job.track_id.clone(),
        outcome.map_err(|e| e.to_string()),
    )))
}

fn run_analysis(
    conn: &Connection,
    track_id: Option<&str>,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    let track_id = track_id.ok_or_else(|| Error::Invalid("analysis job without track".into()))?;
    let path = crate::library::track_path(conn, track_id)?;
    if !path.exists() {
        return Err(Error::NotFound(format!("file missing: {}", path.display())));
    }
    let started = now_ms();
    let a = analysis::analyze_file(&path, cancel)?;
    analysis::store(conn, track_id, &a, now_ms() - started)
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueSummary {
    pub queued: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
    pub cancelled: i64,
}

pub fn summary(conn: &Connection) -> Result<QueueSummary> {
    let mut s = QueueSummary::default();
    let mut stmt = conn.prepare("SELECT status, count(*) FROM jobs GROUP BY status")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (status, n) = row?;
        match status.as_str() {
            "queued" => s.queued = n,
            "running" => s.running = n,
            "done" => s.done = n,
            "failed" => s.failed = n,
            "cancelled" => s.cancelled = n,
            _ => {}
        }
    }
    Ok(s)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRow {
    pub id: i64,
    pub kind: String,
    pub track_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub status: String,
    pub attempts: i64,
    pub error: Option<String>,
    pub updated_at: i64,
}

pub fn recent(conn: &Connection, status: Option<&str>, limit: i64) -> Result<Vec<JobRow>> {
    let mut stmt = conn.prepare(
        "SELECT j.id, j.kind, j.track_id, t.title, t.artist, j.status, j.attempts, j.error, j.updated_at
         FROM jobs j LEFT JOIN tracks t ON t.id = j.track_id
         WHERE (?1 IS NULL OR j.status = ?1)
         ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END, j.updated_at DESC, j.id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status, limit], |r| {
        Ok(JobRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            track_id: r.get(2)?,
            title: r.get(3)?,
            artist: r.get(4)?,
            status: r.get(5)?,
            attempts: r.get(6)?,
            error: r.get(7)?,
            updated_at: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn cancel_queued(conn: &Connection) -> Result<usize> {
    let n = conn.execute(
        "UPDATE jobs SET status = 'cancelled', updated_at = ?1 WHERE status = 'queued'",
        [now_ms()],
    )?;
    conn.execute(
        "UPDATE tracks SET analysis_status = CASE WHEN EXISTS (SELECT 1 FROM analysis_results a WHERE a.track_id = tracks.id) THEN 'done' ELSE 'none' END
         WHERE analysis_status = 'queued'",
        [],
    )?;
    Ok(n)
}

pub fn retry_failed(conn: &Connection) -> Result<usize> {
    let n = conn.execute(
        "UPDATE jobs SET status = 'queued', attempts = 0, error = NULL, updated_at = ?1 WHERE status = 'failed'",
        [now_ms()],
    )?;
    conn.execute(
        "UPDATE tracks SET analysis_status = 'queued' WHERE analysis_status = 'failed'",
        [],
    )?;
    Ok(n)
}

pub fn clear_finished(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM jobs WHERE status IN ('done','cancelled')", [])?)
}

// ---------------------------------------------------------------------------
// Worker pool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: i64,
    pub track_id: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
}

/// Analysis profile: how many worker threads to use.
pub fn workers_for_profile(profile: &str) -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    match profile {
        "light" => 1,
        "maximum" => cpus.saturating_sub(1).max(1),
        _ => (cpus / 2).max(1),
    }
}

pub struct WorkerPool {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    active: Arc<AtomicUsize>,
}

impl WorkerPool {
    /// Start `n` workers, each with its own SQLite connection (WAL mode).
    pub fn start(
        db_path: PathBuf,
        n: usize,
        on_event: Arc<dyn Fn(JobEvent) + Send + Sync>,
    ) -> WorkerPool {
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let handles = (0..n.max(1))
            .map(|i| {
                let (stop, paused, active, path, on_event) = (
                    stop.clone(),
                    paused.clone(),
                    active.clone(),
                    db_path.clone(),
                    on_event.clone(),
                );
                std::thread::Builder::new()
                    .name(format!("dc-worker-{i}"))
                    .spawn(move || {
                        let conn = match crate::db::open(&path) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!("worker {i}: cannot open db: {e}");
                                return;
                            }
                        };
                        let cancel = || stop.load(Ordering::Relaxed);
                        while !stop.load(Ordering::Relaxed) {
                            if paused.load(Ordering::Relaxed) {
                                std::thread::sleep(Duration::from_millis(300));
                                continue;
                            }
                            active.fetch_add(1, Ordering::SeqCst);
                            let r = run_one(&conn, &cancel);
                            active.fetch_sub(1, Ordering::SeqCst);
                            match r {
                                Ok(Some((job_id, track_id, outcome))) => on_event(JobEvent {
                                    job_id,
                                    track_id,
                                    ok: outcome.is_ok(),
                                    error: outcome.err(),
                                }),
                                Ok(None) => std::thread::sleep(Duration::from_millis(400)),
                                Err(e) => {
                                    eprintln!("worker {i}: {e}");
                                    std::thread::sleep(Duration::from_millis(1000));
                                }
                            }
                        }
                    })
                    .expect("spawn worker")
            })
            .collect();
        WorkerPool {
            stop,
            paused,
            handles,
            active,
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn busy(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    pub fn size(&self) -> usize {
        self.handles.len()
    }

    /// Stop workers; an in-flight analysis is interrupted and re-queued.
    pub fn shutdown(self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles {
            let _ = h.join();
        }
    }
}
