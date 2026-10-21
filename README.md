# Deep Cuts

**Free, private, local DJ library intelligence** for Windows and macOS (Linux builds too).

Deep Cuts imports your music, analyses it on your own machine, and helps you triage, organise, search,
clean and export your library, with Rekordbox interoperability. No account, no subscription, no cloud,
no remote AI.

> Status: **v0.1.x: all planned local features usable; analysis accuracy is the current focus.** See `docs/STATUS.md` for exactly what works,
> what is verified and what is not built yet. The app never shows simulated AI results.

## What works today

* **Import** folders (recursive, drag-and-drop or dialog): MP3, WAV, AIFF, FLAC, ALAC/AAC (M4A), OGG.
  Import is read-only and incremental. Moved files are relinked by content.
* **Library** with a virtualised table (100k+ tracks), sortable/custom columns, multi-select,
  context menu, playlists, and a details inspector that shows where every value came from
  (file / Rekordbox / analysis / you). Single and bulk editing; any edit can be reverted.
* **Keep / Pass / Maybe** triage with keyboard shortcuts (`K` `P` `M` `U`, ratings `1`–`5`,
  auto-advance) and contextual decisions.
* **Preview player**: waveform, seek, HOT preview (measured most-energetic region), intro/outro,
  next/previous. Decoded fallback for formats the webview can't play.
* **Local audio analysis** (pure DSP, background queue that survives restarts): BPM, beat phase, key
  (Camelot/Open Key), EBU R128 loudness, peak/crest, spectral brightness and low end, onset rate,
  energy curve, waveform.
* **Search language** shared with **Smart Crates**:
  `rolling bpm:122-126 key:8A+ genre:"tech house" -decision:pass (label:a OR label:b)`.
* **Library Health**: missing files, duplicates (identical files, same artist + title, same recording
  by acoustic fingerprint across formats), fake-lossless/transcode suspects, playlists with missing
  files, unused files. **Merge duplicates** keeps one copy, re-points playlists and moves the others
  to a trash folder. Nothing is ever deleted.
* **DJ software**: import and export **Rekordbox XML**, **Traktor NML**, **Serato crates** and
  **VirtualDJ** (tracks, playlists and folders, BPM, key, rating, comments, cues, loops, beatgrid
  where the format has them); **M3U8** and **CSV** export.
* **Rebuild external playlists offline**: Spotify (Exportify CSV), Apple Music text exports, M3U or
  "Artist - Title" tracklists matched to your files, with review and a missing-tracks list.
* **Music DNA**: energy 1–10 within your library, genre and tag suggestions learned from your own
  labels (with measured accuracy), automatic groups; **Discover** (similar tracks, map);
  **Set Builder** (chapters, Magic Sort, next-track suggestions, build a set from one track).
* **Bulk tools**: find & replace (regex), CSV metadata import with column mapping, rename/move files
  from metadata, playlist merge/common/subtract/shuffle/prefix, archive, cover art add/remove,
  relocate the library to another drive, **recipes** (saved search + actions, also on import).
* **Undo / redo** (Ctrl/⌘+Z) for edits, decisions, tags and smart fixes, plus per-track change history.
* **Tags**: hierarchical tags, synonyms, categories, Quick Tag (Shift+1…9), Tag Editor.
* **Smart Fixes** with preview: URLs/promo text, track-number prefixes, SHOUTED CAPS, feat./separators,
  genre spelling, invisible characters, version/remixer extraction.
* **Cue points, loops, beatgrid**: phrase-aligned cue suggestions (Intro/Main/Breakdown/Drop/Outro),
  memory/hot cues at the playhead (`C`), beat-snapped loops, beatgrid editor with tap tempo.
* **Write tags to files** on request (dry run, copies of originals, read-back verification).
* **Watch folders** for automatic import.
* **Backups**: daily automatic snapshot, manual backups, automatic snapshot before risky operations,
  restore with safety copy, integrity check.
* **Dashboard** computed from your real library. **English and Italian** UI.
* `deepcuts` **CLI** on the same database (import, search, decide, analyze, tag, fix, write-tags, rename, import-dj, export-dj, match-playlist, undo, export, stats, eval, bench).

## Build and run

Requirements: Rust (stable), Node.js 22. On Linux, also the Tauri system packages
(`libwebkit2gtk-4.1-dev librsvg2-dev`).

```bash
npm install          # also installs ui/ dependencies
npm run dev          # desktop app with hot reload
npm run build        # installers in target/release/bundle (MSI/NSIS, DMG, deb/AppImage)
npm test             # UI unit tests + all Rust tests
cargo run -p dc-cli --release -- --help    # CLI
```

The library lives in the OS data directory (`%APPDATA%\DeepCuts`, `~/Library/Application Support/DeepCuts`,
`~/.local/share/DeepCuts`). Set `DEEPCUTS_HOME` to use another location.

## Repository layout

```
crates/dc-core   Rust core: database, import/export, provenance, analysis, jobs, backups
crates/dc-cli    `deepcuts` command-line tool
src-tauri        desktop shell (IPC commands, audio protocol)
ui               React + TypeScript interface
docs             architecture, feature matrix, test plan, benchmarks, licensing, status
```

## Documentation

* [Status report](docs/STATUS.md)
* [Competitor feature matrix](docs/COMPETITOR_FEATURE_MATRIX.md)
* [Architecture & database](docs/ARCHITECTURE.md) · [Scope](docs/SCOPE.md)
* [Test plan](docs/TEST_PLAN.md) · [Analysis/AI validation](docs/AI_VALIDATION.md) · [Benchmarks](docs/BENCHMARKS.md)
* [Licensing](docs/LICENSING.md) · [Model research](docs/MODEL_RESEARCH.md) · [Legacy migration](docs/LEGACY_MIGRATION.md)

## Licence

GPL-3.0-or-later. See `LICENSE` and `docs/LICENSING.md`.
