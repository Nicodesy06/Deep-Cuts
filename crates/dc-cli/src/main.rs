//! `deepcuts` — headless access to a Deep Cuts library (same database as the
//! desktop app). Useful for scripting, verification and benchmarks.

use clap::{Parser, Subcommand};
use dc_core::query::{TrackQuery, query_track_ids, query_tracks};
use dc_core::{
    analysis, backup, cues, db, export, fixes, history, jobs, library, paths, rekordbox, stats,
    tags, tagwrite,
};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "deepcuts", version, about = "Deep Cuts library tool")]
struct Cli {
    /// Library database (default: the desktop app's library)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Import (or refresh) audio files from folders/files. Read-only on files.
    Import { paths: Vec<PathBuf> },
    /// Search the library with the query language (see `deepcuts help-query`).
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        sort: Option<String>,
        #[arg(long)]
        desc: bool,
        #[arg(long, default_value_t = 50)]
        limit: i64,
        #[arg(long)]
        json: bool,
    },
    /// Set keep/pass/maybe (or "none" to clear) for tracks matching a query.
    Decide {
        decision: String,
        query: String,
        #[arg(long, default_value = "global")]
        context: String,
    },
    /// Queue and run audio analysis for tracks matching a query.
    Analyze {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        force: bool,
        /// light | balanced | maximum
        #[arg(long, default_value = "balanced")]
        profile: String,
    },
    /// Show stored analysis for one track id.
    Show { id: String },
    /// Export tracks matching a query as m3u8, csv or rekordbox xml.
    Export {
        out: PathBuf,
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value = "Deep Cuts Export")]
        name: String,
    },
    /// Import a Rekordbox XML collection (backup taken first).
    ImportRekordbox { xml: PathBuf },
    /// Library statistics as JSON.
    Stats,
    /// Mark files that no longer exist.
    CheckMissing,
    /// Create a database backup.
    Backup,
    /// List backups.
    Backups,
    /// Database integrity check.
    Check,
    /// Benchmark on a synthetic library of N tracks (separate database).
    Bench {
        #[arg(default_value_t = 100_000)]
        tracks: usize,
        #[arg(long, default_value = "bench-out")]
        dir: PathBuf,
    },
    /// Describe the query language.
    HelpQuery,
    /// Add (or with --remove, remove) a tag on tracks matching a query.
    Tag {
        name: String,
        query: String,
        #[arg(long, default_value = "custom")]
        category: String,
        #[arg(long)]
        remove: bool,
    },
    /// List tags with track counts.
    Tags,
    /// Preview smart fixes for tracks matching a query; --apply to apply all.
    Fix {
        #[arg(default_value = "")]
        query: String,
        /// Comma-separated rule ids (default: all)
        #[arg(long)]
        rules: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Show what would be written into the audio files; --yes to write.
    WriteTags {
        query: String,
        #[arg(long)]
        yes: bool,
        /// Do not keep copies of the original files.
        #[arg(long)]
        no_backup: bool,
    },
    /// Undo the last change batch.
    Undo,
    /// Redo the last undone batch.
    Redo,
    /// Recent change batches.
    History,
    /// Watch folders: list, add <path>, remove <path>.
    Watch {
        action: String,
        path: Option<PathBuf>,
    },
    /// Show cue points (incl. suggestions) for a track id.
    Cues { id: String },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> dc_core::Result<()> {
    let db_path = cli.db.clone().unwrap_or_else(paths::db_path);
    if let Cmd::Bench { tracks, dir } = &cli.cmd {
        return bench(*tracks, dir);
    }
    if let Cmd::HelpQuery = &cli.cmd {
        println!("{}", QUERY_HELP);
        return Ok(());
    }
    let mut conn = db::open(&db_path)?;
    let backups = db_path
        .parent()
        .map(|p| p.join("backups"))
        .unwrap_or_else(paths::backups_dir);
    match cli.cmd {
        Cmd::Import { paths } => {
            let t = Instant::now();
            let rep = library::import_paths(&mut conn, &paths, None, &mut |p| {
                if p.total > 0 {
                    eprint!("\r{} {}/{}", p.phase, p.done, p.total);
                }
            })?;
            eprintln!();
            println!(
                "scanned {} | added {} | updated {} | unchanged {} | relinked {} | failed {} | {:.1}s",
                rep.scanned,
                rep.added,
                rep.updated,
                rep.unchanged,
                rep.relinked,
                rep.failed.len(),
                t.elapsed().as_secs_f64()
            );
            for f in &rep.failed {
                println!("  failed: {} ({})", f.path, f.error);
            }
        }
        Cmd::Search {
            query,
            sort,
            desc,
            limit,
            json,
        } => {
            let page = query_tracks(
                &conn,
                &TrackQuery {
                    query,
                    sort,
                    desc,
                    limit: Some(limit),
                    ..Default::default()
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&page)?);
            } else {
                println!("{} tracks", page.total);
                for t in page.tracks {
                    println!(
                        "{}  {:<28} {:<36} {:>7} {:>4} {:>6} {}",
                        &t.id[..8],
                        trunc(t.artist.as_deref().unwrap_or("-"), 28),
                        trunc(t.title.as_deref().unwrap_or("-"), 36),
                        t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default(),
                        t.camelot.as_deref().unwrap_or(""),
                        t.decision.as_deref().unwrap_or(""),
                        t.analysis_status
                    );
                }
            }
        }
        Cmd::Decide {
            decision,
            query,
            context,
        } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let d = (decision != "none").then_some(decision.as_str());
            library::set_decision(&conn, &ids, &context, d)?;
            println!("{} tracks -> {decision} ({context})", ids.len());
        }
        Cmd::Analyze {
            query,
            force,
            profile,
        } => {
            jobs::recover_interrupted(&conn)?;
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let rep = jobs::enqueue_analysis(&conn, &ids, force)?;
            println!(
                "queued {} | up to date {} | already queued {}",
                rep.queued, rep.skipped_up_to_date, rep.already_queued
            );
            let total = jobs::summary(&conn)?.queued;
            let n = jobs::workers_for_profile(&profile);
            let t = Instant::now();
            let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let d2 = done.clone();
            let pool = jobs::WorkerPool::start(
                db_path.clone(),
                n,
                std::sync::Arc::new(move |ev: jobs::JobEvent| {
                    let k = d2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    match ev.error {
                        None => eprint!("\ranalysed {k}/{total}"),
                        Some(e) => eprintln!("\nfailed {}: {e}", ev.track_id.unwrap_or_default()),
                    }
                }),
            );
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let s = jobs::summary(&conn)?;
                if s.queued == 0 && s.running == 0 {
                    break;
                }
            }
            pool.shutdown();
            eprintln!();
            println!(
                "{} jobs with {n} workers in {:.1}s",
                done.load(std::sync::atomic::Ordering::SeqCst),
                t.elapsed().as_secs_f64()
            );
        }
        Cmd::Show { id } => {
            let id = resolve_id(&conn, &id)?;
            let t = library::get_track(&conn, &id)?;
            println!("{}", serde_json::to_string_pretty(&t)?);
            println!(
                "fields: {}",
                serde_json::to_string_pretty(&library::field_values(&conn, &id)?)?
            );
            match analysis::latest_result(&conn, &id)? {
                Some(mut r) => {
                    r.energy_curve_db.truncate(8);
                    println!(
                        "analysis (energy curve truncated): {}",
                        serde_json::to_string_pretty(&r)?
                    )
                }
                None => println!("not analysed"),
            }
        }
        Cmd::Export { out, query, name } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let ext = out
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let n = match ext.as_str() {
                "m3u8" | "m3u" => export::export_m3u8(&conn, &ids, &out)?,
                "csv" => export::export_csv(&conn, &ids, &out)?,
                "xml" => export::export_rekordbox_xml(&conn, &ids, &name, &out)?,
                _ => {
                    return Err(dc_core::Error::Invalid(
                        "output must end in .m3u8, .csv or .xml".into(),
                    ));
                }
            };
            println!("exported {n} tracks to {}", out.display());
        }
        Cmd::ImportRekordbox { xml } => {
            let b = backup::create(&conn, &backups, "pre-rekordbox-import")?;
            println!("backup: {}", b.path);
            let rep = rekordbox::import_xml(&mut conn, &xml)?;
            println!("{}", serde_json::to_string_pretty(&rep)?);
        }
        Cmd::Stats => println!(
            "{}",
            serde_json::to_string_pretty(&stats::library_stats(&conn)?)?
        ),
        Cmd::CheckMissing => {
            let (n, m) = library::check_missing_files(&conn)?;
            println!("{n} files checked, {m} missing");
        }
        Cmd::Backup => println!("{}", backup::create(&conn, &backups, "manual")?.path),
        Cmd::Backups => {
            for b in backup::list(&backups)? {
                println!("{}  {:>10} bytes  {}", b.file_name, b.size, b.reason);
            }
        }
        Cmd::Check => println!("{}", db::integrity_check(&conn)?.join("\n")),
        Cmd::Tag {
            name,
            query,
            category,
            remove,
        } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let tag = tags::ensure(&conn, &name, &category)?;
            let n = tags::assign(&conn, &ids, &tag, !remove)?;
            println!("{n} tracks changed ({} matched)", ids.len());
        }
        Cmd::Tags => {
            for t in tags::list(&conn)? {
                println!("{:<12} {:<32} {:>6}", t.category, t.name, t.track_count);
            }
        }
        Cmd::Fix {
            query,
            rules,
            apply,
        } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let rules: Vec<String> = match rules {
                Some(r) => r.split(',').map(|x| x.trim().to_string()).collect(),
                None => fixes::RULES.iter().map(|r| r.id.to_string()).collect(),
            };
            let props = fixes::preview(&conn, &ids, &rules, 100_000)?;
            for p in &props {
                println!(
                    "{:<8} {:<10} {:?} -> {:?}  [{}]",
                    &p.track_id[..8],
                    p.field,
                    p.old_value.as_deref().unwrap_or(""),
                    p.new_value.as_deref().unwrap_or(""),
                    p.rules.join(",")
                );
            }
            if apply {
                let accepted: Vec<fixes::AcceptedFix> = props
                    .iter()
                    .map(|p| fixes::AcceptedFix {
                        track_id: p.track_id.clone(),
                        field: p.field.clone(),
                        new_value: p.new_value.clone(),
                    })
                    .collect();
                println!(
                    "applied {} fixes (undo with `deepcuts undo`)",
                    fixes::apply(&conn, &accepted)?
                );
            } else {
                println!("{} proposals (preview only; --apply to apply)", props.len());
            }
        }
        Cmd::WriteTags {
            query,
            yes,
            no_backup,
        } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let plan = tagwrite::plan(&conn, &ids)?;
            for p in &plan {
                println!("{}", p.path);
                for c in &p.changes {
                    println!(
                        "    {:<12} {:?} -> {:?}",
                        c.field,
                        c.file_value.as_deref().unwrap_or(""),
                        c.new_value.as_deref().unwrap_or("")
                    );
                }
            }
            if !yes {
                println!(
                    "{} files would change (dry run; --yes to write)",
                    plan.len()
                );
            } else {
                let dir = backups.join("audio-originals");
                let rep = tagwrite::write(&mut conn, &ids, (!no_backup).then_some(dir.as_path()))?;
                println!("{}", serde_json::to_string_pretty(&rep)?);
            }
        }
        Cmd::Undo => match history::undo(&conn)? {
            Some(b) => println!(
                "undone: {} ({} changes)",
                b.summary.unwrap_or(b.action),
                b.changes
            ),
            None => println!("nothing to undo"),
        },
        Cmd::Redo => match history::redo(&conn)? {
            Some(b) => println!(
                "redone: {} ({} changes)",
                b.summary.unwrap_or(b.action),
                b.changes
            ),
            None => println!("nothing to redo"),
        },
        Cmd::History => {
            for b in history::recent(&conn, 30)? {
                println!(
                    "#{:<5} {:<10} {:<40} {:>4} changes{}",
                    b.id,
                    b.action,
                    b.summary.unwrap_or_default(),
                    b.changes,
                    if b.undone { "  (undone)" } else { "" }
                );
            }
        }
        Cmd::Watch { action, path } => match (action.as_str(), path) {
            ("list", _) => {
                for w in library::watch_folders(&conn)? {
                    println!(
                        "{}{}",
                        w.path,
                        if w.exists { "" } else { "  (unavailable)" }
                    );
                }
            }
            ("add", Some(p)) => println!(
                "watching {} (the desktop app picks it up)",
                library::add_watch_folder(&conn, &p)?
            ),
            ("remove", Some(p)) => library::remove_watch_folder(&conn, &p.to_string_lossy())?,
            _ => {
                return Err(dc_core::Error::Invalid(
                    "usage: watch list | add <path> | remove <path>".into(),
                ));
            }
        },
        Cmd::Cues { id } => {
            let id = resolve_id(&conn, &id)?;
            for c in cues::list(&conn, &id)? {
                println!(
                    "{:>9.3}s  {:<10} {:<3} {:<12} {}{}",
                    c.start_ms / 1000.0,
                    c.kind,
                    c.slot
                        .map(|s| ((b'A' + s as u8) as char).to_string())
                        .unwrap_or_default(),
                    c.name.unwrap_or_default(),
                    c.source,
                    if c.locked { " (locked)" } else { "" }
                );
            }
        }
        Cmd::Bench { .. } | Cmd::HelpQuery => unreachable!(),
    }
    Ok(())
}

fn resolve_id(conn: &dc_core::rusqlite::Connection, prefix: &str) -> dc_core::Result<String> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM tracks WHERE id LIKE ?1 || '%' LIMIT 2")?;
        let rows = stmt.query_map([prefix], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    match ids.len() {
        1 => Ok(ids[0].clone()),
        0 => Err(dc_core::Error::NotFound(prefix.into())),
        _ => Err(dc_core::Error::Invalid(format!("'{prefix}' is ambiguous"))),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.into()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

/// Synthetic large-library benchmark: inserts tracks through the same code
/// paths as a real import (field provenance + FTS), then times queries.
fn bench(n: usize, dir: &std::path::Path) -> dc_core::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("bench.db");
    let _ = std::fs::remove_file(&path);
    let conn = db::open(&path)?;
    let genres = [
        "Tech House",
        "Deep House",
        "Techno",
        "Minimal",
        "Melodic Techno",
        "UK Garage",
        "Disco",
        "Trance",
    ];
    let keys = [
        "Am", "Em", "Bm", "F#m", "C", "G", "D", "Fm", "Cm", "Gm", "Bb", "Eb",
    ];
    let t = Instant::now();
    let mut rng: u64 = 42;
    let mut next = || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (rng >> 33) as usize
    };
    let mut ids = Vec::with_capacity(n);
    for chunk in 0..n.div_ceil(1000) {
        let tx = dc_core::util::write_tx(&conn)?;
        for i in chunk * 1000..((chunk + 1) * 1000).min(n) {
            let id = dc_core::util::new_id();
            let now = dc_core::util::now_ms();
            tx.execute(
                "INSERT INTO tracks (id, created_at, updated_at) VALUES (?1, ?2, ?2)",
                dc_core::rusqlite::params![id, now],
            )?;
            tx.execute(
                "INSERT INTO files (id, track_id, path, size, format, last_seen, added_at) VALUES (?1, ?2, ?3, 1000, 'mp3', ?4, ?4)",
                dc_core::rusqlite::params![dc_core::util::new_id(), id, format!("/music/bench/{i}.mp3"), now],
            )?;
            let fields = [
                ("artist", format!("Artist {}", next() % 5000)),
                ("title", format!("Track {i} groove {}", next() % 100)),
                ("genre", genres[next() % genres.len()].to_string()),
                ("bpm", format!("{}", 110 + next() % 30)),
                ("key", keys[next() % keys.len()].to_string()),
                ("label", format!("Label {}", next() % 800)),
                ("durationMs", format!("{}", 300_000 + next() % 200_000)),
            ];
            for (f, v) in fields {
                library::set_field(&tx, &id, f, library::SOURCE_FILE, Some(&v))?;
            }
            library::refresh_fts(&tx, &id)?;
            ids.push(id);
        }
        tx.commit()?;
    }
    println!(
        "insert {n} tracks (provenance + FTS): {:.2}s",
        t.elapsed().as_secs_f64()
    );
    library::set_decision(&conn, &ids[..n / 10], "global", Some("keep"))?;
    let queries = [
        "",
        "groove",
        "artist:\"Artist 42\"",
        "bpm:123-125 key:8A",
        "genre:tech -decision:keep",
        "key:8A+ bpm:>120 (genre:techno OR genre:minimal)",
    ];
    for q in queries {
        for sort in [None, Some("bpm")] {
            let t = Instant::now();
            let page = query_tracks(
                &conn,
                &TrackQuery {
                    query: q.into(),
                    sort: sort.map(String::from),
                    limit: Some(200),
                    ..Default::default()
                },
            )?;
            println!(
                "query {:<52} sort {:<5} -> {:>6} hits, first page in {:>6.1} ms",
                format!("{q:?}"),
                sort.unwrap_or("-"),
                page.total,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    let t = Instant::now();
    let s = stats::library_stats(&conn)?;
    println!(
        "dashboard stats ({} tracks): {:.1} ms",
        s.total_tracks,
        t.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

const QUERY_HELP: &str = r#"Query language (search bar, Smart Crates, CLI):
  words / "phrase"            full-text (prefix) over title, artist, version, remixer,
                              album, label, genre, comment, notes, path
  artist:foo title:"exact"    text fields: artist title album label genre subgenre
                              version remixer comment notes path format
  bpm:124  bpm:122-126  bpm:>120  year:<2000  rating:>=4  duration:>360  lufs:-12--6
  key:8A  key:Am  key:8A+     (+ = harmonically compatible on the Camelot wheel)
  decision:keep|pass|maybe|none  fav:yes  missing:yes  analyzed:no  artwork:no
  tag:name  added:7d   none = empty value (genre:none bpm:none key:none)
  AND (implicit), OR, NOT / -term, ( ... )"#;
