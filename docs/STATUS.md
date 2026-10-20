# Status report — v0.1.0 (first functional build)

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

* Tags are never written back to audio files yet (deliberate, for data safety).
* Editing a Smart Crate recreates it (new id).
* Waveform/analysis memory scales with track length (~56 MB per 5-minute track per worker).
* AIFF/ALAC preview on Windows relies on the decoded-WAV fallback (first play of a long file takes
  a moment).
* No cue editor, tag editor UI, smart fixes, watch folders or undo stack yet.
* Discover, Music DNA, Set Builder and Tag Editor pages are clearly marked *planned*. They contain no
  simulated results.

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
