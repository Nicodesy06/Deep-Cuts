# Test plan

Run everything: `npm install && npm test` (UI unit tests + `cargo test --workspace`).

## Automated tests in the repository

| Area | Test | Where | Input |
|---|---|---|---|
| Import (folders, recursive, non-audio ignored, broken files reported) | `import_decide_search_export_restart_analyze` | `crates/dc-core/tests/library_flow.rs` | Synthesised WAVs + a corrupt MP3 |
| Tag reading for MP3, FLAC, AIFF, AAC-M4A, ALAC-M4A | `encoded_formats_with_tags` | same | ffmpeg-encoded files with tags (skipped with a message when ffmpeg is absent) |
| Metadata persistence across restart | step 5 of the flow test | same | reopened database |
| Keep/Pass/Maybe incl. contextual decisions | steps 3, 10 | same | |
| Search language (text, fields, ranges, OR/NOT, keys) | flow test + `query::tests` | | |
| User edits vs analysis vs import precedence | step 7 + Rekordbox test | | |
| Incremental re-import, track relocation/relink | steps 8–9 | | moved + renamed file |
| Duplicate candidates (never deletes) | `duplicate_candidates` | | identical copy at another path |
| Smart playlists | step 11 | | |
| M3U8 / CSV export | step 4 | | |
| Rekordbox XML export → import round trip, idempotent re-import, missing files kept | `rekordbox_xml_round_trip` | | |
| Rekordbox XML parsing (entities, cues, loops, folders) | `rekordbox::tests` | | inline XML |
| Backup / restore (+ safety snapshot, integrity check) | `backup_and_restore` | | |
| Job queue persistence and crash recovery | `interrupted_jobs_are_recovered_once` | | |
| Analysis: tempo + key sweep, silence, too-short, cancellation | `tests/analysis_synthetic.rs` | | exact synthetic ground truth |
| Loudness calibration (BS.1770) | `analysis::loudness::tests` | | 1 kHz sine |
| Resampler | `audio::tests` | | sine |
| Key notation / Camelot compatibility | `keys::tests` | | |
| Audio protocol: ranges, 416, WAV fallback, no path traversal | `audio_protocol::serve_tests` | `src-tauri` | |
| UI: formatting, selection model, i18n parity, virtual table, histograms | `ui/src/**/*.test.ts(x)` | vitest + jsdom | |

## Not yet automated (manual or pending)

| Area | Plan |
|---|---|
| Cross-platform build + tests | GitHub Actions matrix (`.github/workflows/ci.yml`): Ubuntu, Windows, macOS (Apple Silicon). First run pending. |
| Packaging (MSI/NSIS, DMG) | `npm run build` on each OS in CI. Signing/notarisation in Phase 9. |
| Audible playback per platform | Manual check on Windows (WebView2) and macOS (WKWebView): MP3, WAV, FLAC, AIFF, M4A. The headless Linux container has no audio device. |
| Rekordbox fidelity | Import exported XML into Rekordbox 6/7, compare fields, cues, grids. Then round-trip a real collection. |
| Large libraries (real files) | Import 100k real files from a network or USB drive; measure time and memory. |
| UI end-to-end | WebDriver (tauri-driver) suite in Phase 9. |

## AI / analysis evaluation

See `docs/AI_VALIDATION.md`.
