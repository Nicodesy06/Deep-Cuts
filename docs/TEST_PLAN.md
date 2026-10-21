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
| Undo/redo (edits, decisions, notes, favourites, tags), no-op edits not recorded, redo stack cleared by new actions | `undo_redo_edits_decisions_notes_favorites` | `crates/dc-core/tests/phase2.rs` | |
| Tags: hierarchy, synonyms, cycles refused, toggle, quick slots, AI suggestion review, delete re-parents | `hierarchical_tags_synonyms_and_quick_slots` | same | |
| Smart fixes: rules (unit), preview is read-only, partial apply, undo | `fixes::tests`, `smart_fixes_preview_apply_undo` | | |
| Cue suggestions on the phrase grid; user cues protected across re-analysis; accept → locked hot cue; suggestions not exported | `cues::tests`, `analysis_creates_phrase_cue_suggestions_and_user_cues_are_protected` | | synthetic track with a −16 dB breakdown |
| Tag writing: plan, changed-on-disk refusal, backups, read-back, DB refresh, audio still decodes | `write_tags_back_to_files` | | MP3, FLAC, AAC-M4A, AIFF via ffmpeg |
| File rename/move from metadata: plan, conflicts, cross-volume moves, undo/redo | `tests/fileops.rs`, `fileops::tests` | | |
| Transcode detection (fake FLAC / fake 320k flagged; genuine and honest files not) | `tests/transcode.rs`, `bandwidth::tests` | | real LAME/FLAC encodes via ffmpeg |
| Acoustic duplicates across formats, gain and trims; other recordings not grouped | `tests/fingerprint.rs` | | ffmpeg encodes of broadband synthetic music |
| Merge duplicates, archive, broken playlists, unused files | `tests/dedupe.rs` | | |
| Find & replace (plain/word/regex, preview, undo); playlist combine/shuffle/prefix/occurrences | `tests/bulk_tools.rs` | | |
| CSV metadata import (artist+title and path matching, validation); relocate + undo | `tests/csv_relocate.rs`, `relocate::tests` | | also run with a symlinked TMPDIR to mimic macOS `/var` → `/private/var` |
| Rekordbox / Traktor / Serato / VirtualDJ export → re-import round trip | `tests/dj_apps.rs` | | |
| Energy rank, genre/tag suggestions with leave-one-out accuracy, auto groups | `tests/dna.rs` | | |
| Beatgrid edit, loops, undo; cover art add/remove with backups | `phase2::user_beatgrid_edit_loop_and_undo`, `phase2::album_art_add_and_remove_safely` | | |
| Set from a seed track over the transition graph | `phase3::graph_path_from_seed…` | | |
| Recipes on demand and on import, undo | `tests/recipes.rs` | | |
| External playlist matching (CSV/M3U/text parsing, version-aware scoring, playlist creation) | `tests/playlist_match.rs`, `playlist_match::tests` | | |
| Schema migrations (v1 → v8) | manual on the v1 test library + `db::migrate` on every open | | |
| UI: formatting, selection model, i18n parity, virtual table, histograms, tag tree | `ui/src/**/*.test.ts(x)` | vitest + jsdom | |

## Not yet automated (manual or pending)

| Area | Plan |
|---|---|
| Cross-platform build + tests | GitHub Actions matrix (`.github/workflows/ci.yml`): Ubuntu, Windows, macOS. **Passing** since run #2. |
| Watch folders (OS events) | Manual: add a folder, copy/rename/delete files while the app runs, check import/relink. Automated test pending. |
| Packaging (MSI/NSIS, DMG) | `npm run build` on each OS in CI. Signing/notarisation in Phase 9. |
| Audible playback per platform | Manual check on Windows (WebView2) and macOS (WKWebView): MP3, WAV, FLAC, AIFF, M4A. The headless Linux container has no audio device. |
| DJ software fidelity | Load exported Rekordbox XML, Traktor NML, Serato crates and VirtualDJ playlists into the real applications; compare fields, cues, grids. Then round-trip a real collection. |
| Large libraries (real files) | Import 100k real files from a network or USB drive; measure time and memory. |
| UI end-to-end | WebDriver (tauri-driver) suite in Phase 9. |

## AI / analysis evaluation

See `docs/AI_VALIDATION.md`.
