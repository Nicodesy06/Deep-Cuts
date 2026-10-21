# Status report

## Increment 6 — Online tagging, exports, cue templates, audio model (2026-10-21)

| Feature | Where | Verification |
|---|---|---|
| **BPM range** setting (fold the estimate by octaves into 60–120 … 98–195) and **comparison with Rekordbox/other imported values** (exact / close / octave / other; key same / fifth / relative / parallel) | `analysis::tempo`, `compare`, Settings, Analysis Queue | `tempo::tests`, `tests/compare.rs`, GUI check |
| **Key notation rewrite** in file tags (original, Camelot, Open Key, musical); Camelot **energy moves** +2 / +7 labelled and searchable (`key:8A++`) | `tagwrite`, `similarity`, `query` | `tests/key_notation.rs`, `query::tests` |
| **Cue templates and anchors** (first beat, main, n-th breakdown/drop, outro, end, bar offsets) | `cues`, Cue template dialog | `tests/cue_templates.rs`, GUI check |
| **Exports**: smart crates as VirtualDJ filter rules, field mapping for the exported comment/genre, set chapters as separate crates, **HTML and PDF** track lists | `rules`, `djlib`, `vdj`, `printable` | `rules::tests`, `tests/export_options.rs` |
| **Rekordbox My Tag** imported from the comment convention (`/* a / b */`) into a "My Tag" tag category | `mytag`, Rekordbox import | `tests/mytag.rs` |
| **Online auto-tagger** (One Tagger-like): MusicBrainz, Deezer, iTunes, Discogs (personal token); fuzzy match with duration check, originals preferred over compilations, per-field selection, review before writing, undoable | `autotag`, Auto-tag dialog | `tests/autotag_apply.rs`, `tests/autotag_live.rs` (network, ignored by default, passing) |
| **Local audio model** (PANNs MobileNetV1, AudioSet): genre, mood, vocals, instruments per track; **character** (bass weight, rhythmic drive, groove steadiness); GPU through CUDA / DirectML / CoreML, CPU fallback; can be switched off | `ml`, `dna::character`, Music DNA, track details | `tests/ml.rs`, CI `genre-eval`, GUI check |

After v0.1.1: optional **Discogs style model** (Discogs-EffNet, 400 Discogs styles; CC BY-NC-SA,
downloaded only after the user accepts the licence; asked once before the first analysis). Measured
on Beatport genres: 42 % top-1 / 75 % top-3 (vs 34–36 % before). Its styles are shown per track,
can be applied as genres, and replace the sound vector for similarity and label suggestions.
Fixed: most UI settings (BPM range, export mapping, model toggle, key notation, auto-tagger options)
were rejected by the backend and silently not saved; a test now checks every key the UI writes.

v0.2.0 (released with installers for macOS, Windows and Linux; website: https://nicodesy06.github.io/Deep-Cuts/): the Discogs pack also gives feel descriptors (danceability, moods, voice, engagement),
MSD tags and arousal/valence; optional **accurate styles** with MAEST (61 % / 88 % top-1/top-3 vs
38 % / 80 % for EffNet on the same 143 tracks; slower); optional **text search by description**
(LAION-CLAP, Discover › Describe; Precision@10 20 % on genre queries, chance 4 %); My Tags read
from comments on import, M3U8 import/export. Evaluated and not adopted: Beat This! for tempo
(worse than the current estimator on Beatport BPM labels). Not integrated: Demucs, All-In-One.

v0.2.1: fixes from the independent technical audit (see [AUDIT_RESPONSE.md](AUDIT_RESPONSE.md)):
journalled, recoverable file moves and all-or-nothing merges; tag writes through a verified staged
copy with a refresh journal; watched-folder removal marks every child; watcher errors shown in the
app; automatic schema-upgrade tests from every old version with a pre-upgrade copy; portable file
names; listening confirmation for non-identical duplicate merges; capped HTTPS-only model
downloads and model removal; dependency audit in CI; SHA-256 checksums on releases.

After v0.1.1: Rekordbox My Tags in comments (`/* a / b */`) are read automatically when files are
imported or change, and by "Scan all comments"; M3U/M3U8 playlists (e.g. from Rekordbox) import as
playlists, adding missing files; every DJ export gets a "My Tag" folder with one playlist per tag,
and one M3U8 per tag can be written. Fixed: FLAC/OGG comments were not read at all.

Released as **v0.1.1** (pre-release: Windows x64, macOS Apple silicon, macOS Intel without the
model). Audit and fixes after the first release (`FEATURE_AUDIT.md`): the model's embedding now gives
each track a 32-dim sound vector used by similar tracks and by genre/tag suggestions (Beatport
test set: genre suggestions 34 % → 42 % top-1, same-genre share in the top 10 30 % → 36 %); a
Beatport genre classifier was trained and measured (36 %) and is not shipped; the Intel Mac build
is produced without the model.

Platform verification: CI now installs the real NSIS (Windows), DMG (macOS) and deb (Linux)
packages and runs them in smoke mode (import, analysis with the bundled model, playback in the
platform webview); see `TEST_PLAN.md`. Fixed while doing it: on Linux, WebKitGTK refused to play
media from the `dcaudio` scheme, so the player now loads it as a blob there. The bundled ONNX
Runtime needs glibc ≥ 2.38 on Linux (Ubuntu 24.04+, Debian 13+, Fedora 39+); CI builds on 24.04.

Not done: reading My Tag directly from Rekordbox 6/7's encrypted `master.db` (needs the database
key; left to the owner's decision), and checks with real speakers and in real DJ apps (checklist in
`TEST_PLAN.md`).

## Increment 5 — Library tools, DJ software interop, Music DNA, automation (2026-10-21)

Goal of this increment: every planned local feature in a usable state before improving the analysis.

| Feature | Where | Verification |
|---|---|---|
| **Merge duplicates** safely (keep one copy, re-point playlists, carry decision/tags/rating/notes, other files to a dated trash folder, archive copies, undoable); **archive** (hidden, `archived:yes`); broken playlists; unused files in a folder | `dedupe`, Library Health, Library menu | `tests/dedupe.rs`, GUI check |
| **Find & replace** (plain / whole word / regex), **playlist tools** (merge, common, subtract, duplicate, shuffle, prefix, occurrences) | `bulk`, `playlist_tools` | `tests/bulk_tools.rs`, GUI check |
| **CSV metadata import** with column mapping and validation; **relocate library** (path prefix mapping between drives/OS) | `csvimport`, `relocate`, Sync & Export, Settings | `tests/csv_relocate.rs`, GUI check |
| **Traktor, Serato, VirtualDJ** import/export; whole-library Rekordbox export with folders and smart-crate snapshots | `djlib`, `traktor`, `serato`, `vdj`, `deepcuts import-dj/export-dj` | `tests/dj_apps.rs` (round trip through every format) |
| **Music DNA**: energy 1–10 (library rank of the measured energy proxy), genre and tag suggestions learned from the user's own labels (accuracy measured leave-one-out and shown), automatic groups | `dna`, Music DNA page | `tests/dna.rs`, GUI check |
| **Beatgrid editor and loops** (downbeat at playhead, nudge, BPM ×2/÷2, tap tempo, zoomed view, beat-snapped 4/8/16-beat loops; user grid wins on export; undoable) | `grid`, `GridEditor` | `phase2::user_beatgrid_edit_loop_and_undo`, GUI check |
| **Build a set from a track** (beam search over the transition graph) | `sets::build_path`, Library menu | `phase3::graph_path_from_seed…`, GUI check |
| **Recipes**: saved search + actions, run on demand or on import (watch folders too), one undoable step | `recipes`, Smart Crates | `tests/recipes.rs`, GUI check (run + undo) |
| **Cover art** add/replace/remove in files, with backups and read-back verification | `artwork`, Library menu | `phase2::album_art_add_and_remove_safely` |
| **Rebuild external playlists offline** (Exportify/Spotify CSV, Apple Music text, M3U, "Artist - Title" lists): fuzzy match with version awareness, review, playlist creation, missing list | `playlist_match`, Sync & Export, `deepcuts match-playlist` | `tests/playlist_match.rs`, `playlist_match::tests`, GUI check |

### Out of scope for the local version (justified)

Engine DJ / Pioneer USB / OneLibrary writing (undocumented binary formats, risk of corrupting a
stick used live — supported path: Rekordbox XML → Rekordbox → USB), direct streaming-service
import (online, account, API terms — the offline matcher covers exported playlists), online
auto-tagging from catalogues, cloud sync, mobile apps and a local network API (the app makes no
network requests; the CLI covers scripting). Rekordbox `master.db` (encrypted) is not read.

### Fixed while testing

- Relocate test failed on macOS/Windows: the old prefix was canonicalised after the folder had
  been renamed. The destination is now stored in canonical form too, so later scans match.
- Playlist matcher: another version/edit of a song is never auto-accepted (offered for review).
- Smart-crate hint promised a natural-language generator that does not exist; text corrected.

### Analysis improvements (dc-dsp 1.3.0)

| Change | Measured |
|---|---|
| **Key**: reference-tuning estimate + new front-end and learned profiles (chosen on GiantSteps-MTG-Keys) | GiantSteps Key MIREX **64.8 % → 67.5 %**, exact **55.5 % → 59.3 %** |
| **BPM precision**: 16-beat comb + snap to whole BPM when the signal supports it (chosen on half of the Beatport-labelled set) | Exact BPM (±0.05) on the held-out Beatport half **84.0 % → 86.9 %**, Accuracy1/2 unchanged there and on GiantSteps (CI run 39540659685) |
| Tempo octave prior | Not adopted again: +2.6 Acc1 on Beatport labels but −4 Acc2 on GiantSteps |

GiantSteps Tempo labels turned out to be perceptual and imprecise (the two published versions differ
by > 0.5 BPM on 379 of 541 tracks), so they are used for octave/gross errors only.
Existing analyses are recomputed when the user runs *Analyze* (analyser 1.3.0); user values win.

### Next priorities

1. Tempo octave decision that keeps GiantSteps Accuracy2 (e.g. a learned half/double decision).
2. Cue/structure and energy: find annotated corpora to measure them the same way.
3. Manual smoke test of installers on Windows/macOS; load exports in the real DJ applications.

---

## Increment 4 — Key estimation 1.1.0, rename & move files (2026-10-20)

| Feature | Where | Verification |
|---|---|---|
| **Key estimation 1.1.0**: new chroma front-end + EDM key profiles, chosen on a development corpus and measured on a separate test corpus. GiantSteps Key MIREX **47.5 % → 64.8 %**, exact key **34.6 % → 55.5 %** | `analysis::key`, `keystudy`, `deepcuts key-study`, `eval.yml` job `key-study` | `AI_VALIDATION.md`; synthetic key tests still exact |
| **Transcode detection** (fake lossless / fake high bitrate): measured bandwidth cliff at the source sample rate, stated rule, Library Health list, `transcode:yes` search, bandwidth metric in details; codec recorded per file (schema v4, analyser 1.2.0) | `analysis::bandwidth`, `library::transcode_suspects`, migration `0004` | `tests/transcode.rs` (real LAME encodes), `bandwidth::tests`, GUI check |
| **Duplicates by sound** across formats/bitrates: acoustic fingerprint computed during analysis (schema v5), same-recording groups in Library Health next to identical-file and same-artist+title groups | `analysis::fingerprint`, `library::acoustic_pairs` | `tests/fingerprint.rs` (FLAC / MP3 320 / MP3 128 / AAC / −6 dB / trimmed grouped; other recordings not; threshold from measured error rates), GUI check |
| **Tempo study** (576 variants, dev/test): best development variant not adopted (test Accuracy2 −4 points); production tempo unchanged | `tempostudy`, `deepcuts tempo-study` | `AI_VALIDATION.md` |
| **Rename / move files from metadata**: pattern with tokens, dry-run preview with conflicts (missing, name clash, existing file, no metadata), never overwrites, verified cross-drive moves, one undoable batch (undo moves files back) | `dc-core::fileops`, `history::Kind::FilePath`, Library context menu, `deepcuts rename` | `tests/fileops.rs`, `fileops::tests`, GUI check (rename + undo on real files) |

Existing analyses (1.0.0/1.1.0) are recomputed when the user runs *Analyze*; keys the user set by
hand are never overwritten (user values win over analysis).

`cargo test --workspace --release`: **64 passed** · UI: **18 passed** · clippy: **0 warnings**.

---

## Increment 3 — Discover, Set Builder, real-music evaluation (2026-10-20)

### Implemented in this increment

| Feature | Where | Verification |
|---|---|---|
| **Measured feature vectors** (17 dims: tempo, Camelot circle, mode, loudness, crest, spread, brightness, low/high band share, onset rate, flux, periodicity, energy mean/std/slope, breakdown depth) stored per analysed track, backfilled for older analyses | `dc-core::features`, schema v3 `track_features` | `features::tests`, `phase3::discovery_map_projects_all_analysed_tracks` |
| **Similar tracks** with per-group explanation (tempo, harmonic relation, loudness, timbre, rhythm, energy), adjustable weights, query scope, user key corrections respected | `dc-core::similarity`, Discover → Similar, `deepcuts similar` | `phase3::similar_tracks_are_ranked_and_explained`, GUI check |
| **Discovery map** (PCA of the z-scored vectors; axes show explained variance and top loadings; colour by key/decision) | `dc-core::map`, Discover → Map | `phase3::discovery_map…`, GUI check |
| **Set Builder**: set playlists with chapters (opening/warm-up/build/peak/cool-down/closing), pinned tracks, per-transition report (BPM step, harmonic relation, energy step), **Magic Sort** (smooth / build / wave) with pins kept, undoable apply, next-track suggestions that exclude set members and passes, set notes | `dc-core::sets`, `ui/views/SetBuilder.tsx`, `deepcuts set` | `phase3::set_builder_chapters_locks_sort_suggest_and_undo`, `sets::tests`, GUI check |
| **Playlist order changes** (add, remove, reorder, sort) recorded in the undo history | `library::reorder_playlist`, `history::Kind::PlaylistOrder` | same test |
| **Real-music evaluation** of tempo and key on GiantSteps (CI workflow `eval.yml`, `deepcuts eval`) | `dc-core::eval`, `scripts/fetch_giantsteps.sh` | results in `AI_VALIDATION.md` |

### Measured accuracy (dc-dsp 1.0.0, GiantSteps)

Tempo Accuracy1 **65.8 %**, Accuracy2 **88.2 %** (v2 annotations). Key MIREX score **47.5 %**
(exact 34.6 %). Key detection is weak and is the top analysis priority; see `AI_VALIDATION.md`.

### Test totals (this increment, local)

`cargo test --workspace --release`: **54 passed** · UI (vitest): **18 passed** · clippy: **0 warnings**.

### Next priorities

1. Tempo: a development set with perceptual tempo labels, or a learned beat tracker (same dev/test protocol).
2. Manual smoke test of installers on Windows/macOS.
3. Acoustic-fingerprint duplicates and transcoding detection; file move/rename with undo.
4. ONNX Runtime + model registry for Music DNA; loops/beatgrid editing.

---

## Increment 2 — Phase 2 essentials (2026-10-20)

### Implemented in this increment

| Feature | Where | Verification |
|---|---|---|
| **Undo / redo** of edits, bulk edits, decisions, notes, favourites, tags, smart fixes (Ctrl/⌘+Z, Ctrl/⌘+Shift+Z, sidebar buttons, `deepcuts undo/redo/history`) | `dc-core::history` (schema v2 `change_batches`/`changes`) | `phase2::undo_redo_edits_decisions_notes_favorites` |
| **Per-track change history** in the details panel | `history::track_history` | same test + GUI |
| **Hierarchical tags** with categories, synonyms, Quick Tag slots (Shift+1…9), AI-suggestion review path; Tag Editor section; tag chips + add-tag in details; tags in the context menu; `tag:` search covers children and synonyms | `dc-core::tags`, `ui/views/TagEditor.tsx` | `phase2::hierarchical_tags_synonyms_and_quick_slots`, `TagEditor.test.ts`, GUI check |
| **Smart Fixes** (10 rules: invisible chars, URLs/promo, track-number prefixes, feat., separators, SHOUTED CAPS, genre spelling, whitespace, version/remixer extraction) with preview, per-row accept, one undoable batch | `dc-core::fixes`, Library Health | `fixes::tests` (rules), `phase2::smart_fixes_preview_apply_undo`, GUI check |
| **Cue editor** (memory/hot cue at playhead with `C`, rename, delete, lock) and **phrase-aligned cue suggestions** (Intro / Main / Breakdown / Drop / Outro on 8-bar phrases) created by every analysis; suggestions shown on the waveform, never exported until accepted | `dc-core::cues` | `cues::tests`, `phase2::analysis_creates_phrase_cue_suggestions…` (synthetic breakdown at 60 s found within one beat; user cue survives re-analysis) |
| **Write tags to files** (title, artist, album, album artist, label, genre, remixer, year, BPM, key, comment, ISRC): dry-run plan, refusal of files changed on disk, optional copy of originals, read-back verification, automatic restore on failure | `dc-core::tagwrite`, dialog in Library context menu and Sync & Export | `phase2::write_tags_back_to_files` (MP3, FLAC, AAC-M4A, AIFF; audio still decodes; incremental re-import sees no change) |
| **Watch folders** (OS notifications + 3 s debounce + startup catch-up; removed paths marked missing so moves relink) | `src-tauri/src/watch.rs`, Settings | registry test + manual run on Linux (copy → imported and queued for analysis; rename → same track relinked; delete → marked missing). No automated test of the OS event loop. |
| Schema **v2** migration (from v1, in place) | `migrations/0002_phase2.sql` | ran on the v1 test library: integrity `ok`, data intact |
| CLI: `tag`, `tags`, `fix`, `write-tags`, `undo`, `redo`, `history`, `watch`, `cues` | `crates/dc-cli` | manual runs on the test library |

### Fixes found while testing this increment

* Beat phase could land one period late when the first kick starts at 0 s → phase wrap.
* "Outro" suggestion could coincide with "Drop" → nearest free phrase boundary.
* SHOUTED titles with a normal-case version suffix were not fixed → per-word rule when caps dominate.
* Genre normalisation merged distinct genres (Melodic Techno → Melodic House & Techno) → now
  spelling-only, with a test that distinct genres are never merged.
* ID3v2/MP4 have only an integer BPM frame → a rounded file BPM counts as in sync (documented).

### Cross-platform

CI run #2 (commit `51fa3be`) passed on **Ubuntu 22.04, Windows (latest) and macOS (latest)**: UI
typecheck + tests, all Rust tests, and unsigned installer builds (Windows ~10.7 MB, macOS ~6.4 MB
artifacts). On the Windows runner the ffmpeg-dependent format tests are skipped (no ffmpeg).
Audible playback on Windows/macOS still needs a manual check on real hardware.

### Test totals (this increment, local)

`cargo test --workspace --release`: **39 passed** · UI (vitest): **18 passed** · clippy: **0 warnings**.

### Next priorities

1. Manual smoke test of the installers on Windows and macOS (audible preview, folder dialog, drag-and-drop).
2. Real-corpus evaluation of tempo/key/cue suggestions (`AI_VALIDATION.md`).
3. Playlist/cue edits in the undo history; file move & rename with relink and undo; loops/beatgrid editing UI.
4. Acoustic-fingerprint duplicates and transcoding detection.
5. ONNX Runtime + model registry (schema v3) and CLAP embedding evaluation for similar-track search.

---

## Increment 1 — v0.1.0 (first functional build)

Date: 2026-10-20. Everything listed as passing was actually run in the development container
(Linux x86-64). Windows and macOS have **not** been run yet. The CI matrix that does so is committed
but its first run is pending.

## First-execution path (master prompt §29, step 4)

| # | Step | Result |
|---|---|---|
| 1 | Start the application | ✅ Desktop app built (`tauri build --debug`) and run under Xvfb. All 11 sections render (screenshots taken). |
| 2 | Choose a folder | ✅ Native folder dialog + OS drag-and-drop wired. The dialog itself was not clicked in the headless run. Import was driven via CLI/tests on the same core. |
| 3 | Import real audio files | ✅ MP3, FLAC, AIFF, AAC-M4A, ALAC-M4A, WAV (tests + 36-file test library) |
| 4 | Read metadata | ✅ Tags via lofty, file-name fallback, version/remixer extraction |
| 5 | Show library | ✅ Virtualised table, sorting, columns, details with provenance (screenshot) |
| 6 | Playback | ⚠️ Streaming protocol verified by tests (ranges, WAV fallback), and the player UI loads tracks. **Audible output not verified**: the container has no audio device. |
| 7 | Keep/Pass/Maybe | ✅ Keyboard + buttons (GUI checked with xdotool), contextual decisions |
| 8 | Save decisions | ✅ SQLite, transactional, audit-logged |
| 9 | Search | ✅ Full-text + query language (GUI checked: `key:7A+ -decision:pass`) |
| 10 | Export playlist | ✅ M3U8, CSV, Rekordbox XML |
| 11 | Restart without data loss | ✅ Test reopens the database and checks decisions/playlists. The app reopened the same library across runs. |
| 12 | First authentic analysis job | ✅ Background worker pool: tempo, beat phase, key, loudness, spectral features, waveform |
| 13 | Show the result | ✅ Details panel "Measured analysis", BPM/key columns, waveform in player |
| 14 | Automated tests | ✅ 25 Rust tests (core, analysis, protocol) + 16 UI tests, all passing |

## Build & test results (this run)

* `cargo test --workspace --release`: **25 passed, 0 failed**.
* `npm --prefix ui test`: **16 passed**. `tsc --noEmit`: clean. `vite build`: OK.
* `cargo clippy --workspace --all-targets`: **0 warnings**.
* `npx tauri build --debug --no-bundle` (Linux): OK. Installer bundling not run locally (CI).

## Bugs found and fixed by real use during this increment

1. Beat phase locked onto off-beat hi-hats → phase now estimated from the kick-band envelope.
2. Nested transaction error in Rekordbox import → `util::atomic` joins an open transaction.
3. Unfiltered first page took 465–736 ms at 100k tracks → two-phase paging, now ~40 ms.
4. `database is locked` with parallel workers → `BEGIN IMMEDIATE` for all write transactions.
5. Tracks analysed twice because each worker connection re-queued "running" jobs → explicit,
   owner-aware startup recovery (`jobs::recover_interrupted`) with a regression test.
6. Library table overflowed its pane and the header didn't scroll horizontally → sticky header
   inside the scroll container, slimmer default columns.

## Unverified components

* Windows (WebView2) and macOS (WKWebView) builds, installers and audible playback.
* Analysis accuracy on real music (only synthetic ground truth so far; see `AI_VALIDATION.md`).
* Rekordbox XML export loaded into actual Rekordbox; import of a real large Rekordbox XML.
* Import of 100k real files from disk (query performance at 100k *is* measured).
* HTML5 drag of tracks onto playlists on Windows (Tauri's file-drop handler can intercept it).
  The context menu "Add to playlist" works everywhere.

## Known issues / limitations

* Editing a Smart Crate recreates it (new id).
* Waveform/analysis memory scales with track length (~56 MB per 5-minute track per worker).
* AIFF/ALAC preview on Windows relies on the decoded-WAV fallback (first play of a long file takes
  a moment).
* Music DNA is the only page still marked *planned*. It contains no simulated results.

## Feature parity snapshot

From `COMPETITOR_FEATURE_MATRIX.md`: P0 library-management items are **PARTIALLY VERIFIED**
(implemented and tested on Linux). Music DNA, similarity/discovery, set building, other-platform
adapters, tag writing and acoustic-fingerprint duplicates are **NOT STARTED / IN DESIGN**. Live
performance, streaming-service and production features are **OUT OF SCOPE — JUSTIFIED**.
Crate Intelligence Pro is **BLOCKED** (product not found).

## Next priorities

1. **Run CI on Windows + macOS.** Fix platform issues, verify audible preview and installers.
2. **Real-corpus analysis evaluation** (GiantSteps tempo/key + own library) → publish numbers, then
   tune or replace stages (Beat This! for beats/downbeats).
3. **Phase 2 essentials:** tag writing with backup + preview, smart fixes, watch folders, cue editor,
   undo stack, tag editor UI, acoustic-fingerprint duplicates.
4. **Model runtime:** ONNX Runtime integration + model registry migration, CLAP embeddings
   evaluation for similar-track search (Phase 5 groundwork).
5. Legacy `doureallywanttokeepit?` importer once the repository is shared.
