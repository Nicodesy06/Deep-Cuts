# Feature audit: premium features replicated (2026-10-21)

For each reference app, what Deep Cuts replicates and how well, judged only on evidence: automated
tests, measurements on public annotated corpora (`AI_VALIDATION.md`), the installer smoke tests
run on real Windows / macOS / Linux CI machines, and GUI checks. Nothing here was tested on the
owner's own hardware or inside the real DJ applications yet (checklist in `TEST_PLAN.md`).

Legend: **OK** works and is verified · **WEAK** works, with a measured or known limitation ·
**UNVERIFIED** implemented and tested in isolation, never checked against the real third-party
software · **NO** not implemented (reason given).

## Platforms

| Item | Verdict | Evidence / limitation |
|---|---|---|
| Windows 10/11 x64 (NSIS, MSI) | OK | CI installs the NSIS package, imports MP3/M4A/FLAC/WAV/AIFF, analyses with the model, plays every file in WebView2 |
| macOS Apple silicon (DMG) | OK | Same smoke test; model runs on CoreML |
| macOS Intel (DMG) | WEAK | ONNX Runtime has no Intel-Mac binaries: that build has no audio model (no model genres/moods/sound similarity) |
| Linux x64 (deb) | OK | Smoke test passes; needs glibc ≥ 2.38. Fixed: WebKitGTK refused `dcaudio` media (now played as a blob) |
| GPU inference | UNVERIFIED | DirectML/CUDA code paths exist; CI machines have no GPU, so only CPU (Windows/Linux) and CoreML (macOS) were exercised |

## Lexicon

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Convert library to Rekordbox / Traktor / Serato / VirtualDJ | UNVERIFIED | Round trip through every format keeps tracks, playlists, BPM, key, rating, cues (`tests/dj_apps.rs`); never loaded into the real apps |
| Smart playlist conversion | WEAK | Rules only for VirtualDJ (filter text); Rekordbox XML has no smart lists, Traktor/Serato formats undocumented → snapshots |
| Key conversion, key notation rewrite in files | OK | All 24 keys × 3 notations; bulk rewrite tested |
| Streaming playlist import | OK | Offline matching of Exportify/Apple/M3U/text lists (`tests/playlist_match.rs`); no streaming API |
| Field mappings, CSV/M3U8/HTML/PDF export | OK | `tests/export_options.rs` (PDF read back) |
| Player, waveform, hot/intro/outro preview | OK | Playback verified on the three OS by the smoke test |
| Editing, tags, recipes, smart lists, archive, incoming | OK | Core tests + GUI checks |
| Write tags / album art to files | OK | MP3, FLAC, M4A, AIFF; audio still decodes; backups |
| Undo history | OK | Edits, decisions, cues, renames, merges |
| Cues, loops, beat grid editing; cue templates & anchors | OK | Tests + GUI |
| Automatic cue / section detection | WEAK | Correct on synthetic tracks; accuracy on real music **not measured** (no annotated corpus with audio yet) |
| BPM analysis | OK | Beatport hold-out: Accuracy1 95.1 %, Accuracy2 97.8 %, exact ±0.05 BPM 89.1 % |
| Key analysis | WEAK | GiantSteps Key: MIREX 67.5 %, exact 59.3 %; about one track in three gets a wrong or related key |
| Energy / mood fields | WEAK | Energy 1–10 is the library rank of a measured proxy, not validated against human ratings; moods from the model are not validated |
| Find lost/broken/unused files, smart fixes, playlist tools, duplicates | OK | Tests (`dedupe`, `fixes`, `bulk_tools`, `fingerprint`) |
| Watch folder | WEAK | Manual run on Linux only; no automated OS-event test |
| Move & rename files, path mappings, CSV tag import | OK | `tests/fileops.rs`, `tests/csv_relocate.rs` |
| Backup | OK | Local snapshots; cloud storage out of scope |
| Pioneer USB / DIRECT2CDJ / Last.fm / sending to streaming services / mobile app | NO | Out of scope for the offline core (documented in the matrix) |

## DJOID

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Chapter Builder, chapters exported as crates | OK | `phase3` + `tests/export_options.rs` |
| Graph Playlists | WEAK | "Build a set from this track" and next-track suggestions work; no visual graph; "emotion" not modelled |
| Scatter Map | OK | 2-D projection of all analysed tracks |
| Magic Sorting | OK | Hand-designed cost (not learned); build mode ρ > 0.9 in tests |
| Auto Group | OK | k-means on measured features + model sound vector |
| Genre Detection | WEAK | From the user's own labels: 42 % top-1 on Beatport genres (34 % before the sound vector), confident suggestions on 22 % of tracks at 66 %. Model alone gives broad AudioSet genres (34–41 % agreement); Beatport sub-genres (Tech/Progressive/Deep House…) are **not** reliably recognised |
| Matching Tracks | OK | Measured features + sound vector; same-genre share in the top 10 from 30 % to 36 % on a test set |
| Migration incl. Rekordbox tags | WEAK | Rekordbox XML (My Tags from the comment convention), Traktor, Serato, VirtualDJ. Reading My Tags from Rekordbox 6/7's encrypted `master.db` is not done (needs the owner's decision) |

## MIXO

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Import from DJ apps, export to DJ apps | UNVERIFIED | Format round trips pass; real apps not tried |
| Playlists, smart lists, fast search | OK | 100k-track benchmark: first filtered page 0.3–120 ms |
| Related track finder, duplicate finder, find & replace, playlist matcher | OK | Tests |
| Device sync, Engine/USB export, mobile apps | NO | Out of scope |

## Mixed In Key

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Key & BPM detection | WEAK / OK | See Lexicon rows: BPM strong, key below commercial level |
| Camelot wheel, compatible keys, energy boost +2 / +7 | OK | Tests, `key:8A++` |
| Energy level 1–10 | WEAK | Relative to the library, not human-calibrated |
| Cue point generation | WEAK | Not measured on real music |
| Write results to tags | OK | |
| Mashups, DAW plug-in | NO | Out of scope |

## One Tagger

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Auto tagger | WEAK | MusicBrainz, Deezer, iTunes, Discogs (token) work live; Beatport, Traxsource, Juno are not used (no public API) — so Beatport genres/BPM come only from those other catalogues |
| Quick Tag shortcuts, rename from template, MP3/AIFF/FLAC/M4A | OK | Tests + GUI |
| Spotify audio features | NO | Online-only data |

## Vibes

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Tags by vibe/energy/mood/era with combined filters | OK | |
| On-device AI that learns your tagging | OK | kNN on measured features + sound vector, accuracy shown on the user's own tags |
| BPM/key/cue analysis on import | OK | Cue suggestions are generated during analysis (accuracy: see above) |
| Set designer | WEAK | List-based Set Builder, not a free canvas |
| Export to Rekordbox/Serato/Traktor | UNVERIFIED | See Lexicon |

## Music Library Doctor

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Read DJ libraries | WEAK | Serato crates, VirtualDJ database, Rekordbox XML / Traktor NML exports; not Rekordbox's `master.db` |
| Duplicates by sound across formats | OK | FLAC/MP3 320/128/AAC/−6 dB/trimmed copies grouped, other recordings not |
| Re-point playlists before trashing | OK | |

## Crate Hackers / DJ.Studio

| Feature | Verdict | Evidence / limitation |
|---|---|---|
| Next-track suggestions, Magic Sorter, harmonic/energy ordering | OK | Documented scoring, not "AI" |
| Pre-made crates, timeline mixing, rendering, stems | NO | Out of scope |

## What to fix next (by impact)

1. Check the exports inside Rekordbox, Traktor, Serato and VirtualDJ on real machines
   (`TEST_PLAN.md` checklist).
2. Key accuracy: 67.5 % MIREX is the weakest core analysis result.
3. Measure cue/section detection on an annotated corpus.
4. Genre: a stronger pretrained music model with a licence that allows redistribution would be
   needed for Beatport sub-genres.
