//! `deepcuts` — headless access to a Deep Cuts library (same database as the
//! desktop app). Useful for scripting, verification and benchmarks.

use clap::{Parser, Subcommand};
use dc_core::query::{TrackQuery, query_track_ids, query_tracks};
use dc_core::{
    analysis, backup, cues, db, export, fixes, history, jobs, library, paths, rekordbox, stats,
    tags, tagwrite,
};
use dc_core::{fileops, sets, similarity};
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
    /// Import another DJ app: rekordbox (.xml), traktor (.nml), serato
    /// (_Serato_ folder), virtualdj (VirtualDJ folder).
    ImportDj { app: String, path: PathBuf },
    /// Match an external playlist (CSV e.g. Spotify/Exportify, M3U, or
    /// "Artist - Title" lines) against the library; with --create, make a
    /// playlist from the confident matches.
    MatchPlaylist {
        file: PathBuf,
        #[arg(long)]
        create: bool,
    },
    /// Export to a DJ app: the whole library with playlists (no --query),
    /// or the tracks matching --query as one playlist. Target: .xml/.nml
    /// file, _Serato_ folder, or a folder for .vdjfolder playlists.
    ExportDj {
        app: String,
        target: PathBuf,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value = "Deep Cuts Export")]
        name: String,
    },
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
    /// Rename/move files from metadata (dry run; --yes to apply, undoable).
    /// Pattern tokens: {artist} {title} {version} {remixer} {album} {label}
    /// {genre} {year} {bpm} {key} {camelot}
    Rename {
        query: String,
        #[arg(long, default_value = fileops::DEFAULT_PATTERN)]
        pattern: String,
        /// Move into this folder instead of renaming in place
        #[arg(long)]
        to: Option<PathBuf>,
        #[arg(long)]
        yes: bool,
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
    /// Tracks most similar to a track id (measured features, explained).
    Similar {
        id: String,
        #[arg(long, default_value = "")]
        scope: String,
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
    /// Sets: `set create <name> <query>`, `set show <name>`, `set sort <name> [--mode build] [--apply]`.
    Set {
        action: String,
        name: String,
        query: Option<String>,
        #[arg(long, default_value = "smooth")]
        mode: String,
        #[arg(long)]
        apply: bool,
    },
    /// Evaluate the analyser on an annotated corpus (e.g. GiantSteps):
    /// <stem>.mp3 in AUDIO, <stem>.key in --keys, <stem>.bpm in --tempo.
    Eval {
        audio: PathBuf,
        #[arg(long)]
        keys: Option<PathBuf>,
        #[arg(long)]
        tempo: Option<PathBuf>,
        /// Markdown report path (a .csv with per-track results is written next to it)
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Key-estimator design study. Without --use: score the variant grid on
    /// a development corpus and write the best choice to --choose. With
    /// --use: measure that fixed choice on a test corpus.
    KeyStudy {
        audio: PathBuf,
        /// Directory of <stem>.key files, or a GiantSteps-MTG-Keys
        /// annotations.txt (only confidence-2 labels are used)
        #[arg(long)]
        keys: PathBuf,
        #[arg(long)]
        choose: Option<PathBuf>,
        #[arg(long = "use")]
        use_choice: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "")]
        name: String,
    },
    /// Tempo-estimator design study (same protocol as key-study).
    TempoStudy {
        audio: PathBuf,
        /// Directory of <stem>.bpm files, or a tab-separated metadata file
        /// with an ID column and a "BP BPM" column (GiantSteps-MTG-Keys)
        #[arg(long)]
        tempo: PathBuf,
        #[arg(long)]
        choose: Option<PathBuf>,
        #[arg(long = "use")]
        use_choice: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "")]
        name: String,
    },
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
    if let Cmd::Eval {
        audio,
        keys,
        tempo,
        out,
        name,
    } = &cli.cmd
    {
        return eval_corpus(
            audio,
            keys.as_deref(),
            tempo.as_deref(),
            out.as_deref(),
            name,
        );
    }
    if let Cmd::KeyStudy {
        audio,
        keys,
        choose,
        use_choice,
        out,
        name,
    } = &cli.cmd
    {
        return key_study(
            audio,
            keys,
            choose.as_deref(),
            use_choice.as_deref(),
            out.as_deref(),
            name,
        );
    }
    if let Cmd::TempoStudy {
        audio,
        tempo,
        choose,
        use_choice,
        out,
        name,
    } = &cli.cmd
    {
        return tempo_study(
            audio,
            tempo,
            choose.as_deref(),
            use_choice.as_deref(),
            out.as_deref(),
            name,
        );
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
            for (name, r) in dc_core::recipes::run_on_import(&conn, &rep.added_track_ids)? {
                println!("recipe {name}: {} tracks, {} changes", r.tracks, r.changes);
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
            dc_core::dna::update_energy(&conn)?;
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
        Cmd::ImportDj { app, path } => {
            let b = backup::create(&conn, &backups, &format!("pre-{app}-import"))?;
            println!("backup: {}", b.path);
            let rep = dc_core::djlib::import_from(&mut conn, &app, &path)?;
            println!("{}", serde_json::to_string_pretty(&rep)?);
        }
        Cmd::MatchPlaylist { file, create } => {
            let rep = dc_core::playlist_match::read_and_match(&conn, &file)?;
            for r in &rep.rows {
                let best = r.candidates.first().map(|c| {
                    format!(
                        "{} - {} ({:.2})",
                        c.track.artist.as_deref().unwrap_or("?"),
                        c.track.title.as_deref().unwrap_or("?"),
                        c.score
                    )
                });
                println!(
                    "{:>4} {:<8} {} - {}  =>  {}",
                    r.wanted.line,
                    r.status,
                    r.wanted.artist,
                    r.wanted.title,
                    best.unwrap_or_default()
                );
            }
            println!(
                "{} matched, {} to review, {} missing",
                rep.matched, rep.review, rep.missing
            );
            if create {
                let ids: Vec<String> = rep
                    .rows
                    .iter()
                    .filter(|r| r.status == "matched")
                    .map(|r| r.candidates[0].track.id.clone())
                    .collect();
                dc_core::playlist_match::create_playlist(&conn, &rep.name, &ids)?;
                println!("created playlist '{}' with {} tracks", rep.name, ids.len());
            }
        }
        Cmd::ExportDj {
            app,
            target,
            query,
            name,
        } => {
            let lib = match query {
                None => dc_core::djlib::whole_library(&conn)?,
                Some(q) => {
                    let ids = query_track_ids(
                        &conn,
                        &TrackQuery {
                            query: q,
                            ..Default::default()
                        },
                    )?;
                    dc_core::djlib::for_ids(&conn, &ids, &name)?
                }
            };
            let s = dc_core::djlib::export_to(&app, &lib, &target)?;
            println!("{}", serde_json::to_string_pretty(&s)?);
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
        Cmd::Rename {
            query,
            pattern,
            to,
            yes,
        } => {
            let ids = query_track_ids(
                &conn,
                &TrackQuery {
                    query,
                    ..Default::default()
                },
            )?;
            let plan = fileops::plan(&conn, &ids, &pattern, to.as_deref())?;
            for p in &plan {
                match &p.conflict {
                    Some(c) => println!("skip {}  ({c})", p.from),
                    None => println!("{}\n  -> {}", p.from, p.to),
                }
            }
            let n = plan.iter().filter(|p| p.conflict.is_none()).count();
            if !yes {
                println!("{n} files would be renamed (dry run; --yes to apply)");
            } else {
                let rep = fileops::apply(&conn, &plan)?;
                for f in &rep.failed {
                    eprintln!("failed: {} ({})", f.path, f.error);
                }
                println!(
                    "renamed {} | skipped {} | failed {} (undo with `deepcuts undo`)",
                    rep.moved,
                    rep.skipped,
                    rep.failed.len()
                );
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
        Cmd::Similar { id, scope, limit } => {
            let id = resolve_id(&conn, &id)?;
            for m in
                similarity::similar(&conn, &id, &similarity::Weights::default(), &scope, limit)?
            {
                let groups: Vec<String> = m
                    .groups
                    .iter()
                    .map(|g| {
                        format!(
                            "{}={}",
                            g.group,
                            g.similarity
                                .map(|x| format!("{:.2}", x))
                                .unwrap_or("-".into())
                        )
                    })
                    .collect();
                println!(
                    "{:>3}%  {:<28} {:<36} {:?}  {}",
                    (m.score * 100.0).round(),
                    trunc(m.track.artist.as_deref().unwrap_or("-"), 28),
                    trunc(m.track.title.as_deref().unwrap_or("-"), 36),
                    m.facts.harmonic,
                    groups.join(" ")
                );
            }
        }
        Cmd::Set {
            action,
            name,
            query,
            mode,
            apply,
        } => {
            let find = |conn: &dc_core::rusqlite::Connection| -> dc_core::Result<String> {
                library::list_playlists(conn)?
                    .into_iter()
                    .find(|p| p.kind == "set" && p.name.eq_ignore_ascii_case(&name))
                    .map(|p| p.id)
                    .ok_or_else(|| dc_core::Error::NotFound(format!("set '{name}'")))
            };
            match action.as_str() {
                "create" => {
                    let ids = query_track_ids(
                        &conn,
                        &TrackQuery {
                            query: query.unwrap_or_default(),
                            ..Default::default()
                        },
                    )?;
                    let id = library::create_playlist(&conn, &name, None, "set", None)?;
                    library::reorder_playlist(&conn, &id, &ids, "playlist-add")?;
                    println!("set '{name}' created with {} tracks", ids.len());
                }
                "show" | "sort" => {
                    let id = find(&conn)?;
                    if action == "sort" {
                        let r = sets::magic_sort(&conn, &id, &mode)?;
                        println!("cost {:.2} -> {:.2}", r.cost_before, r.cost_after);
                        if apply {
                            sets::apply_order(&conn, &id, &r.order)?;
                            println!("applied (undo with `deepcuts undo`)");
                        }
                    }
                    let rep = sets::report(&conn, &id, &mode)?;
                    for (i, t) in rep.tracks.iter().enumerate() {
                        println!(
                            "{:>3} {} {:<30} {:<30} {:>7} {:>4}  energy {:>4}  {}",
                            i + 1,
                            if t.locked { "#" } else { " " },
                            trunc(t.track.artist.as_deref().unwrap_or("-"), 30),
                            trunc(t.track.title.as_deref().unwrap_or("-"), 30),
                            t.track.bpm.map(|b| format!("{b:.2}")).unwrap_or_default(),
                            t.track.camelot.as_deref().unwrap_or(""),
                            t.energy
                                .map(|e| format!("{:.0}", e * 100.0))
                                .unwrap_or("-".into()),
                            t.transition
                                .as_ref()
                                .map(|x| format!(
                                    "{:?} {:+.1}% ({:.2})",
                                    x.harmonic,
                                    x.bpm_change_pct.unwrap_or(0.0),
                                    x.score
                                ))
                                .unwrap_or_default()
                        );
                    }
                }
                _ => {
                    return Err(dc_core::Error::Invalid(
                        "set actions: create, show, sort".into(),
                    ));
                }
            }
        }
        Cmd::Bench { .. }
        | Cmd::HelpQuery
        | Cmd::Eval { .. }
        | Cmd::KeyStudy { .. }
        | Cmd::TempoStudy { .. } => {
            unreachable!()
        }
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

fn read_annotation(dir: Option<&std::path::Path>, stem: &str, ext: &str) -> Option<String> {
    let p = dir?.join(format!("{stem}.{ext}"));
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run the analyser over a corpus and score it against annotations.
fn eval_corpus(
    audio: &std::path::Path,
    keys: Option<&std::path::Path>,
    tempo: Option<&std::path::Path>,
    out: Option<&std::path::Path>,
    name: &str,
) -> dc_core::Result<()> {
    use dc_core::eval::{key_relation, summarize_keys, summarize_tempo, tempo_acc1, tempo_acc2};
    use dc_core::keys::Key;
    use rayon::prelude::*;
    let files = library::collect_audio_files(&[audio.to_path_buf()]);
    let started = Instant::now();
    let done = std::sync::atomic::AtomicUsize::new(0);
    struct Row {
        stem: String,
        ref_key: Option<String>,
        est_key: Option<Key>,
        ref_bpm: Option<f64>,
        est_bpm: Option<f64>,
        error: Option<String>,
        secs: f64,
    }
    let rows: Vec<Row> = files
        .par_iter()
        .map(|f| {
            let stem = f
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let ref_key = read_annotation(keys, &stem, "key");
            let ref_bpm = read_annotation(tempo, &stem, "bpm")
                .and_then(|b| b.parse::<f64>().ok())
                .filter(|b| *b > 0.0);
            let t = Instant::now();
            let (est_key, est_bpm, error) = match analysis::analyze_file(f, &|| false) {
                Ok(a) => (
                    a.result.key.as_ref().and_then(|k| Key::parse(&k.name)),
                    a.result.tempo.as_ref().map(|t| t.bpm),
                    None,
                ),
                Err(e) => (None, None, Some(e.to_string())),
            };
            let k = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if k.is_multiple_of(25) {
                eprintln!("{k}/{}", files.len());
            }
            Row {
                stem,
                ref_key,
                est_key,
                ref_bpm,
                est_bpm,
                error,
                secs: t.elapsed().as_secs_f64(),
            }
        })
        .collect();
    let elapsed = started.elapsed().as_secs_f64();

    let key_pairs: Vec<(Option<Key>, Key)> = rows
        .iter()
        .filter_map(|r| {
            r.ref_key
                .as_deref()
                .and_then(Key::parse)
                .map(|k| (r.est_key, k))
        })
        .collect();
    let unparsed_keys = rows
        .iter()
        .filter(|r| {
            r.ref_key
                .as_deref()
                .is_some_and(|k| Key::parse(k).is_none())
        })
        .count();
    let tempo_pairs: Vec<(Option<f64>, f64)> = rows
        .iter()
        .filter_map(|r| r.ref_bpm.map(|b| (r.est_bpm, b)))
        .collect();
    let ks = summarize_keys(&key_pairs);
    let ts = summarize_tempo(&tempo_pairs);
    let failed = rows.iter().filter(|r| r.error.is_some()).count();
    let pct = |v: f64| format!("{:.1} %", v * 100.0);

    let mut md = format!(
        "# Analyser evaluation{}\n\nAnalyser: `{} {}` · files: {} · decode failures: {} · wall time: {:.1} s ({} threads) · mean per file: {:.2} s\n\n",
        if name.is_empty() {
            String::new()
        } else {
            format!(" — {name}")
        },
        analysis::ANALYZER,
        analysis::ANALYZER_VERSION,
        rows.len(),
        failed,
        elapsed,
        rayon::current_num_threads(),
        rows.iter().map(|r| r.secs).sum::<f64>() / rows.len().max(1) as f64,
    );
    if ts.n > 0 {
        md += &format!(
            "## Tempo ({} annotated)\n\n| Metric | Value |\n|---|---|\n| Accuracy1 (±4 %) | {} |\n| Accuracy2 (octave errors allowed) | {} |\n| Mean abs. error on Accuracy1 hits | {:.3} BPM |\n| Exact (±0.05 BPM) | {} |\n| No estimate | {} |\n\n",
            ts.n,
            pct(ts.acc1),
            pct(ts.acc2),
            ts.mae_acc1_hits,
            pct(ts.exact),
            ts.no_estimate
        );
    }
    if ks.n > 0 {
        md += &format!(
            "## Key ({} annotated, {} unparseable labels skipped)\n\n| Metric | Value |\n|---|---|\n| MIREX weighted score | {} |\n| Correct | {} |\n| Fifth | {} |\n| Relative | {} |\n| Parallel | {} |\n| Other | {} |\n| No estimate | {} |\n\n",
            ks.n,
            unparsed_keys,
            pct(ks.weighted),
            pct(ks.correct),
            pct(ks.fifth),
            pct(ks.relative),
            pct(ks.parallel),
            pct(ks.other),
            ks.no_estimate
        );
    }
    println!("{md}");
    if let Some(out) = out {
        export::write_atomic(out, md.as_bytes())?;
        let mut w =
            String::from("file,ref_bpm,est_bpm,acc1,acc2,ref_key,est_key,key_relation,error\n");
        for r in &rows {
            let rk = r.ref_key.as_deref().and_then(Key::parse);
            w += &format!(
                "{},{},{},{},{},{},{},{},{}\n",
                r.stem,
                r.ref_bpm.map(|b| b.to_string()).unwrap_or_default(),
                r.est_bpm.map(|b| format!("{b:.2}")).unwrap_or_default(),
                match (r.est_bpm, r.ref_bpm) {
                    (Some(e), Some(b)) => tempo_acc1(e, b).to_string(),
                    _ => String::new(),
                },
                match (r.est_bpm, r.ref_bpm) {
                    (Some(e), Some(b)) => tempo_acc2(e, b).to_string(),
                    _ => String::new(),
                },
                rk.map(|k| k.camelot()).unwrap_or_default(),
                r.est_key.map(|k| k.camelot()).unwrap_or_default(),
                match (r.est_key, rk) {
                    (Some(e), Some(k)) => format!("{:?}", key_relation(e, k)),
                    _ => String::new(),
                },
                r.error.clone().unwrap_or_default().replace(',', ";"),
            );
        }
        export::write_atomic(&out.with_extension("csv"), w.as_bytes())?;
    }
    Ok(())
}

/// Reference keys by file stem: a directory of `<stem>.key` files, or an
/// MTG-Keys `annotations.txt` (ID, key, confidence; ID = stem up to the first dot).
fn load_key_refs(
    keys: &std::path::Path,
    files: &[PathBuf],
) -> (Vec<(PathBuf, dc_core::keys::Key)>, usize) {
    use dc_core::keys::Key;
    let mut table = std::collections::HashMap::new();
    if keys.is_file() {
        for line in std::fs::read_to_string(keys).unwrap_or_default().lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 3 && cols[2].trim() == "2" {
                table.insert(cols[0].trim().to_string(), cols[1].trim().to_string());
            }
        }
    }
    let mut out = Vec::new();
    let mut unparsed = 0;
    for f in files {
        let stem = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let label = if keys.is_file() {
            table
                .get(stem.split('.').next().unwrap_or_default())
                .cloned()
        } else {
            read_annotation(Some(keys), &stem, "key")
        };
        match label.as_deref().map(Key::parse) {
            Some(Some(k)) => out.push((f.clone(), k)),
            Some(None) => unparsed += 1,
            None => {}
        }
    }
    (out, unparsed)
}

fn key_study(
    audio: &std::path::Path,
    keys: &std::path::Path,
    choose: Option<&std::path::Path>,
    use_choice: Option<&std::path::Path>,
    out: Option<&std::path::Path>,
    name: &str,
) -> dc_core::Result<()> {
    use dc_core::analysis::key;
    use dc_core::keystudy::{self, Choice};
    use rayon::prelude::*;
    let files = library::collect_audio_files(&[audio.to_path_buf()]);
    let (refs, unparsed) = load_key_refs(keys, &files);
    let choice: Option<Choice> = match use_choice {
        Some(p) => Some(
            serde_json::from_slice(&std::fs::read(p)?)
                .map_err(|e| dc_core::Error::Invalid(format!("bad choice file: {e}")))?,
        ),
        None => None,
    };
    let configs = match &choice {
        Some(c) => vec![c.config, key::BASELINE_1_0],
        None => keystudy::grid(),
    };
    let started = Instant::now();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let extracted: Vec<_> = refs
        .par_iter()
        .filter_map(|(f, k)| {
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n.is_multiple_of(50) {
                eprintln!("{n}/{}", refs.len());
            }
            let c = keystudy::extract(f, &configs).ok()?;
            Some((c, *k))
        })
        .collect();
    let failed = refs.len() - extracted.len();
    // Silent tracks have no chroma in any configuration.
    let per_config: Vec<Vec<_>> = (0..configs.len())
        .map(|i| {
            extracted
                .iter()
                .filter_map(|(c, k)| c[i].map(|c| (c, *k)))
                .collect()
        })
        .collect();
    let pct = |v: f64| format!("{:.1} %", v * 100.0);
    let mut md = format!(
        "# Key study{}\n\nAnnotated tracks: {} · unparseable labels skipped: {} · decode failures: {} · {:.0} s\n\n",
        if name.is_empty() {
            String::new()
        } else {
            format!(" — {name}")
        },
        refs.len(),
        unparsed,
        failed,
        started.elapsed().as_secs_f64()
    );
    let row = |s: &dc_core::eval::KeySummary| {
        format!(
            "{} | {} | {} | {} | {} | {}",
            pct(s.weighted),
            pct(s.correct),
            pct(s.fifth),
            pct(s.relative),
            pct(s.parallel),
            pct(s.other)
        )
    };
    match choice {
        Some(c) => {
            let chosen = keystudy::score(&per_config[0], &c.profiles);
            let base = keystudy::score(&per_config[1], &key::KRUMHANSL);
            md += &format!(
                "Fixed choice from the development corpus ({} tracks, dev score {}): `{}` with **{}** profiles.\n\n| Variant | MIREX | Correct | Fifth | Relative | Parallel | Other |\n|---|---|---|---|---|---|---|\n| Chosen | {} |\n| Baseline dc-dsp 1.0.0 | {} |\n\n",
                c.dev_tracks,
                pct(c.dev_weighted),
                c.config.label(),
                c.profile_name,
                row(&chosen),
                row(&base)
            );
        }
        None => {
            let (results, best) = keystudy::study(&configs, &per_config);
            md += "Learned profiles are scored with 5-fold cross-validation.\n\n| Chroma front-end | Profiles | MIREX | Correct | Fifth | Relative | Parallel | Other |\n|---|---|---|---|---|---|---|---|\n";
            let mut lines = Vec::new();
            for r in &results {
                for (p, s) in [
                    ("krumhansl", &r.krumhansl),
                    ("temperley", &r.temperley),
                    ("learned (cv)", &r.learned_cv),
                ] {
                    lines.push((
                        s.weighted,
                        format!("| {} | {} | {} |\n", r.label, p, row(s)),
                    ));
                }
            }
            lines.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, l) in lines {
                md += &l;
            }
            if let Some(b) = best {
                md += &format!(
                    "\n**Choice:** `{}` with {} profiles (dev score {}).\n\nmajor: {:?}\n\nminor: {:?}\n",
                    b.config.label(),
                    b.profile_name,
                    pct(b.dev_weighted),
                    b.profiles.major.map(|v| (v * 1e4).round() / 1e4),
                    b.profiles.minor.map(|v| (v * 1e4).round() / 1e4),
                );
                if let Some(p) = choose {
                    let json = serde_json::to_vec_pretty(&b).unwrap_or_default();
                    export::write_atomic(p, &json)?;
                }
            }
        }
    }
    println!("{md}");
    if let Some(out) = out {
        export::write_atomic(out, md.as_bytes())?;
    }
    Ok(())
}

/// Reference tempi by file stem: a directory of `<stem>.bpm` files, or a
/// tab-separated table with a header containing "ID" and "BP BPM".
fn load_tempo_refs(tempo: &std::path::Path, files: &[PathBuf]) -> Vec<(PathBuf, f64)> {
    let mut table = std::collections::HashMap::new();
    if tempo.is_file() {
        let text = std::fs::read_to_string(tempo).unwrap_or_default();
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
        let col = header.iter().position(|h| h.trim() == "BP BPM");
        if let Some(col) = col {
            for line in lines {
                let cols: Vec<&str> = line.split('\t').collect();
                if let Some(b) = cols.get(col).and_then(|b| b.trim().parse::<f64>().ok()) {
                    table.insert(cols[0].trim().to_string(), b);
                }
            }
        }
    }
    files
        .iter()
        .filter_map(|f| {
            let stem = f.file_stem()?.to_str()?.to_string();
            let bpm = if tempo.is_file() {
                table.get(stem.split('.').next()?).copied()
            } else {
                read_annotation(Some(tempo), &stem, "bpm").and_then(|b| b.parse().ok())
            }?;
            (bpm > 0.0).then(|| (f.clone(), bpm))
        })
        .collect()
}

fn tempo_study(
    audio: &std::path::Path,
    tempo: &std::path::Path,
    choose: Option<&std::path::Path>,
    use_choice: Option<&std::path::Path>,
    out: Option<&std::path::Path>,
    name: &str,
) -> dc_core::Result<()> {
    use dc_core::analysis::tempo as tp;
    use dc_core::tempostudy::{self, Choice};
    use rayon::prelude::*;
    let files = library::collect_audio_files(&[audio.to_path_buf()]);
    let refs = load_tempo_refs(tempo, &files);
    let choice: Option<Choice> = match use_choice {
        Some(p) => Some(
            serde_json::from_slice(&std::fs::read(p)?)
                .map_err(|e| dc_core::Error::Invalid(format!("bad choice file: {e}")))?,
        ),
        None => None,
    };
    let started = Instant::now();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let tracks: Vec<_> = refs
        .par_iter()
        .filter_map(|(f, b)| {
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n.is_multiple_of(50) {
                eprintln!("{n}/{}", refs.len());
            }
            Some((tempostudy::extract(f).ok()??, *b))
        })
        .collect();
    let samples: Vec<_> = tracks.iter().map(|(t, b)| (t, *b)).collect();
    let pct = |v: f64| format!("{:.1} %", v * 100.0);
    let mut md = format!(
        "# Tempo study{}\n\nAnnotated tracks: {} · usable: {} · {:.0} s\n\n",
        if name.is_empty() {
            String::new()
        } else {
            format!(" — {name}")
        },
        refs.len(),
        samples.len(),
        started.elapsed().as_secs_f64()
    );
    let full = |cfg: &tp::TempoConfig, fine: &tp::FineConfig| {
        tempostudy::fine_scores(&samples, cfg, std::slice::from_ref(fine)).remove(0)
    };
    let row = |label: &str, s: &dc_core::eval::TempoSummary| {
        format!(
            "| {label} | {} | {} | {} | {:.3} |\n",
            pct(s.acc1),
            pct(s.acc2),
            pct(s.exact),
            s.mae_acc1_hits
        )
    };
    let header = "| Variant | Accuracy1 | Accuracy2 | Exact (±0.05) | MAE on Acc1 hits (BPM) |\n|---|---|---|---|---|\n";
    match choice {
        Some(c) => {
            let chosen = full(&c.config, &c.fine);
            let base = full(&tp::BASELINE_1_0, &tp::FINE_BASELINE);
            md += &format!(
                "Fixed choice from the development corpus ({} tracks, dev Acc1 {} / Acc2 {} / exact {}): `{}` + `{}`.\n\n{header}",
                c.dev_tracks,
                pct(c.dev_acc1),
                pct(c.dev_acc2),
                pct(c.dev_exact),
                c.config.label(),
                c.fine.label(),
            );
            md += &row("Chosen", &chosen);
            md += &row("Baseline dc-dsp 1.2.0", &base);
        }
        None => {
            let (results, best) = tempostudy::study(&samples);
            let base = tempostudy::score(&samples, &tp::BASELINE_1_0);
            md += &format!(
                "## Octave decision (coarse stage)\n\nBaseline dc-dsp 1.2.0: Acc1 {} / Acc2 {}\n\n| Variant | Accuracy1 | Accuracy2 |\n|---|---|---|\n",
                pct(base.acc1),
                pct(base.acc2)
            );
            for (c, s) in results.iter().take(40) {
                md += &format!("| {} | {} | {} |\n", c.label(), pct(s.acc1), pct(s.acc2));
            }
            if let Some(mut b) = best {
                let (fine_all, fine, fs) = tempostudy::fine_study(&samples, &b.config);
                b.fine = fine;
                b.dev_exact = fs.exact;
                let fine_base = full(&b.config, &tp::FINE_BASELINE);
                md +=
                    &format!("\n## Precision stage (after the chosen octave decision)\n\n{header}");
                md += &row(
                    &format!("Baseline: {}", tp::FINE_BASELINE.label()),
                    &fine_base,
                );
                for (f, s) in fine_all.iter().take(30) {
                    md += &row(&f.label(), s);
                }
                md += &format!(
                    "\n**Choice:** `{}` + `{}` (dev Acc1 {}, Acc2 {}, exact {}).\n",
                    b.config.label(),
                    b.fine.label(),
                    pct(b.dev_acc1),
                    pct(b.dev_acc2),
                    pct(b.dev_exact)
                );
                if let Some(p) = choose {
                    export::write_atomic(p, &serde_json::to_vec_pretty(&b).unwrap_or_default())?;
                }
            }
        }
    }
    println!("{md}");
    if let Some(out) = out {
        export::write_atomic(out, md.as_bytes())?;
    }
    Ok(())
}

const QUERY_HELP: &str = r#"Query language (search bar, Smart Crates, CLI):
  words / "phrase"            full-text (prefix) over title, artist, version, remixer,
                              album, label, genre, comment, notes, path
  artist:foo title:"exact"    text fields: artist title album label genre subgenre
                              version remixer comment notes path format
  bpm:124  bpm:122-126  bpm:>120  year:<2000  rating:>=4  duration:>360  lufs:-12--6
  key:8A  key:Am  key:8A+     (+ = harmonically compatible on the Camelot wheel)
  decision:keep|pass|maybe|none  fav:yes  missing:yes  analyzed:no  artwork:no  transcode:yes
  tag:name  added:7d   none = empty value (genre:none bpm:none key:none)
  AND (implicit), OR, NOT / -term, ( ... )"#;
