# Architecture

```
┌──────────────────────────── Desktop app (Tauri 2) ────────────────────────────┐
│  ui/ (React 19 + TypeScript + Vite)                                            │
│   views: Dashboard · Library · Smart Crates · Library Health · Sync & Export   │
│          Analysis Queue · Settings · (Discover · Music DNA · Set Builder ·     │
│          Tag Editor: planned, shown as such)                                   │
│   ── IPC (invoke/events) ──────────── dcaudio:// protocol (audio preview) ──   │
│  src-tauri/ (Rust): commands.rs · audio_protocol.rs · worker pool lifecycle    │
└───────────────────────────────────────┬────────────────────────────────────────┘
                                        │ same crate, no network
┌───────────────────────────── crates/dc-core (Rust) ────────────────────────────┐
│ db (SQLite, WAL, migrations) · library (tracks, files, provenance, decisions,   │
│ playlists, health) · metadata (lofty) · query (search + smart-crate language)   │
│ audio (symphonia decode, resampler) · analysis (tempo, beat phase, key,         │
│ loudness, spectral, waveform) · jobs (persistent queue + workers) ·             │
│ rekordbox (XML import/export) · export (M3U8, CSV) · backup · stats · keys      │
└───────────────────────────────────────┬────────────────────────────────────────┘
                                        │
                      crates/dc-cli: `deepcuts` (same database; scripting, benchmarks)
```

## Principles

1. **One core, many shells.** All behaviour lives in `dc-core`, which has no UI dependency and is tested
   directly. The desktop app and the CLI are thin shells. A future mobile companion or sync daemon
   reuses the same crate.
2. **Local and offline.** No network code in the app. Audio decoding (Symphonia), tag reading (lofty),
   DSP (rustfft) and storage (bundled SQLite) are all in-process.
3. **Every value has an origin.** See *Metadata provenance* below.
4. **Measured ≠ inferred.** The first analyser (`dc-dsp 1.0.0`) only produces signal measurements. Model
   outputs (Music DNA) will be stored separately with model name, version and validation status.
5. **Non-destructive.** Audio files are opened read-only. Destructive operations (restore) take a
   snapshot first. Nothing is deleted automatically.

## Why this stack

| Choice | Reason | Alternatives considered |
|---|---|---|
| Tauri 2 + React/TS | Small binaries, native webview, Rust backend shared with the core; the stack the brief prefers | Electron (heavier), native per-OS UIs (2× the work) |
| Rust core | Speed for 100k-track libraries and DSP without a Python runtime to ship | Python core: easier ML prototyping, much harder to package for Windows + macOS |
| SQLite (bundled, WAL, FTS5) | Single-file, transactional, fast full-text search, trivially backed up | Embedded KV stores (no SQL for smart crates) |
| Symphonia | Pure-Rust decoders for MP3, AAC, ALAC, FLAC, WAV, AIFF, OGG on every platform | FFmpeg (licensing + packaging weight) |
| Pure DSP first, ONNX later | A real, testable analyser in the first build. Model weights need licence and accuracy validation before they ship. | Shipping a model on day one with unvalidated accuracy |

**Python** is not used at runtime. It stays an option for offline research and model conversion (training,
evaluation, ONNX export). Inference will run in Rust via ONNX Runtime (`ort` crate, CUDA / CoreML / CPU
execution providers), so users never install Python.

## Data model (schema v1, `crates/dc-core/src/migrations/0001_init.sql`)

| Table | Purpose |
|---|---|
| `tracks` | Stable track identity (UUID) + **effective** metadata cached for fast sorting/filtering, `lufs`, `preview_ms`, `analysis_status` |
| `track_fields` | Every metadata value with its `source` (`file`, `import:<app>`, `analysis`, `user`) |
| `files` | Physical files: path, size, mtime, content `quick_hash`, format/codec info, `missing` flag. Many files can map to one track. |
| `external_ids` | IDs in other libraries (e.g. Rekordbox `TrackID`) |
| `decisions` | Keep / Pass / Maybe **per context** (`global` by default, or e.g. a taste profile or a set). A contextual Pass is never a global exclusion. |
| `tags`, `track_tags` | Hierarchical tags with origin, model, version, calibrated confidence (nullable) and status (`suggested`/`confirmed`/`rejected`) |
| `playlists`, `playlist_tracks` | Manual playlists, folders and smart crates (`rules` = query language) |
| `cue_points` | Cues/hot cues/loops with `source` and `locked` (user-confirmed cues are never auto-modified) |
| `beatgrids` | One grid per source (analysis, import, user) |
| `analysis_results` | Versioned raw analyser output (JSON) keyed by analyser version + file content hash |
| `waveforms` | 1200-bin overview (peak, RMS, low band, high band) |
| `jobs` | Persistent job queue (status, attempts, owner process, error) |
| `audit_log` | Every import, edit, decision, relink, playlist deletion |
| `settings` | App settings |
| `tracks_fts` | FTS5 index (title, artist, version, remixer, album, label, genre, comment, notes, path) |

Migrations are ordered SQL files; `PRAGMA user_version` stores the applied version. A database from a
newer build is refused instead of being damaged.

### Metadata provenance

`set_field(track, field, source, value)` stores one row per source. The effective value written to
`tracks` is chosen by precedence:

```
user  >  import:<app>  >  file  >  analysis
```

Consequences, all covered by tests:

* re-importing a folder refreshes `file` values but never touches user edits;
* analysis fills BPM/key only when nothing better exists, and re-analysis never overrides a user value;
* a user edit can be reverted, which restores the next-best value (e.g. the measured BPM);
* Rekordbox imports rank above file tags but below the user.

### File identity and relinking

Tracks are identified by UUID, not path. Each file stores `quick_hash` (BLAKE3 over size + first and
last 256 KiB). If a new path appears whose content matches a file marked missing, the existing file
row is re-pointed (*relink*). Decisions, playlists, cues and analysis all survive the move.

## Query language (search + Smart Crates)

`crates/dc-core/src/query.rs` compiles a small language to parameterised SQL (no string
interpolation of user input):

```
rolling bpm:122-126 key:8A+ genre:"tech house" -decision:pass (label:foo OR label:bar)
```

Full text uses FTS5 prefix queries. Fields support substring, exact (`"…"`), numeric ranges and
comparisons, `none`, harmonic compatibility (`key:8A+`), decisions, flags and recency. Smart crates
store the rule string and resolve it live.

Paging is two-phase. The query first sorts and pages bare ids, then loads display rows only for that
page. This was measured to be ~10× faster at 100k tracks than a single query.

## Analysis pipeline (`dc-dsp 1.0.0`)

1. Decode with Symphonia (corrupt packets skipped and counted) → K-weighted loudness meter on the
   original channels/rate (BS.1770-4 gating) + mono mixdown.
2. Polyphase windowed-sinc resampling to 22 050 Hz.
3. STFT 1024/128: log-magnitude spectral flux (broadband and < 150 Hz), centroid, low/high energy
   ratios, waveform bands.
4. Tempo: FFT autocorrelation of the detrended onset envelope. A comb score over beat multiples does
   a coarse search (60–200 BPM, 0.05 step, log-normal prior around 125 BPM only to settle octave
   ambiguity), then a fine search with 64 multiples (±1.5 %, 0.002 step).
5. Beat phase from the kick-band envelope (DJ grids sit on the kick, while broadband flux often locks
   onto off-beat hats; this was found by a failing test).
6. Key: STFT 8192/4096 chroma (60 Hz–2.1 kHz, triangular semitone weighting) correlated with
   Krumhansl–Kessler profiles. Correlation and margin are reported raw.
7. Energy curve (RMS dB per second) → "hot preview" region = loudest 30 s after the first 10 %.

Each result row stores analyser name + version + file hash. A track is skipped when that triple is
already present (no redundant work). A version bump re-analyses.

## Background work

`jobs::WorkerPool` runs N threads (profile Light = 1, Balanced = cores/2, Maximum = cores−1). Each
thread has its own SQLite connection. Claims are a single atomic `UPDATE … RETURNING` and record the
owning process. At startup, jobs left `running` by a dead process are re-queued (once, explicitly, not
on every connection open: doing it on every open caused double analysis, caught in testing). Writes
use `BEGIN IMMEDIATE`, so concurrent writers wait on the busy timeout instead of failing. Cancelling
or shutting down interrupts decoding and puts the job back in the queue.

## Audio preview

The webview plays `dcaudio://localhost/<track-id>` (Windows: `http://dcaudio.localhost/<id>`). The
protocol resolves the id through the database, so only library files can be served, and it answers
HTTP Range requests (≤ 8 MiB per chunk) for seeking. If the platform webview cannot decode a format
(e.g. AIFF/ALAC in WebView2), the player retries with `?transcode=1` and the core decodes to
16-bit WAV (small LRU cache).

## Security notes

* Tauri capabilities: `core:default` + `dialog:default` only. There is no generic filesystem access
  from the webview.
* CSP restricts media to `self` and the `dcaudio` scheme.
* Backups can only be restored from the Deep Cuts backup directory.
