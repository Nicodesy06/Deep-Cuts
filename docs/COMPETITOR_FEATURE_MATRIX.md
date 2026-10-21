# Competitor Feature Matrix

Scope: **library management, analysis, discovery, curation and set preparation** only.
Mixing, DAW, live performance, stem/mashup production and audio/video rendering are out of scope
by charter (see `docs/SCOPE.md`).

## Sources and how far they were verified (audit of 2026-10-20)

| Competitor | Reference plan | Source used | Confidence |
|---|---|---|---|
| Lexicon | Ultimate (+ Essential, Free) | Official pricing page `lexicondj.com/pricing` (fetched 2026-10-20) | High: per-plan feature lists read from the vendor. Prices were not printed on the page. |
| DJOID | Djoid Pro (€99/year, 1 device) | Official site `djoid.io` (fetched 2026-10-20) | Medium: feature names come from the vendor, some without a description ("Auto Group", "Cue Points", "Genre Detection", "Matching Tracks"). Behaviour also taken from an Evermix review (2026). |
| MIXO | MIXO:GOLD ($7/month) | Official pricing page `mixo.dj/pricing` (fetched 2026-10-20) | High |
| Mixed In Key | Mixed In Key (+ Pro features) | Official site `mixedinkey.com` (fetched 2026-10-20) | Medium: no detailed feature page and no prices on the home page. |
| One Tagger | Free (GPL-3.0) | Third-party listings (linuxlinks, AlternativeTo); project is open source | Medium: verify against the GitHub README before parity work. |
| Vibes | One-time licence | Developer posts on the Native Instruments forum + AlternativeTo/SourceForge listings | Low–medium |
| Music Library Doctor | Pro | AlternativeTo listing only | Low: no official page was reachable. |
| Crate Intelligence Pro | — | **Not found** under this name | Unverified. The closest match is Crate Hackers (below). Needs a link from the product owner. |
| Crate Hackers | Subscription | Third-party review (myweddingsongs.com) | Low. Only local-library features are relevant. |
| DJ.Studio | Ultimate | **Not audited in this pass** | Only selection/ordering features are in scope. |

Legend: **Priority** P0 = first build, P1 = next phases, P2 = later, P3 = optional.
**Status** follows the allowed set: NOT STARTED, IN DESIGN, IMPLEMENTED — UNTESTED, PARTIALLY VERIFIED,
VERIFIED, VERIFIED SUPERIOR, BLOCKED, OUT OF SCOPE — JUSTIFIED.
"PARTIALLY VERIFIED" here means automated tests pass on real audio files, on Linux, Windows and
macOS in CI (format tests needing ffmpeg are skipped on the Windows runner). Manual checks on
real hardware and real-world corpora are still pending.

Module codes: CORE = Library Core (`dc-core::library`), IO = Import/Export (`export`, `rekordbox`),
META = Metadata Engine (`metadata`, field provenance), AN = Audio Analysis (`analysis`), DNA = Music DNA,
SIM = Similarity Search, TASTE = Personal Taste, CRATE = Smart Collections (`query`), SET = Set Builder,
HEALTH = Library Health, BAK = Backup/Recovery (`backup`), UI = desktop app (`ui/`, `src-tauri/`).

---

## Lexicon — Ultimate (verified on the official pricing page)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Lexicon | Free | Library conversion to Rekordbox | Import collection → export Rekordbox-readable library | IO | Rekordbox XML writer | none | XML bridge is a documented Rekordbox feature | P0 | PARTIALLY VERIFIED | `rekordbox_xml_round_trip` (export → import into a fresh library keeps title, rating, BPM, key, genre, hot cue + colour, playlist) | Not yet validated by loading into Rekordbox itself. |
| Lexicon | Free | Conversion to Serato / Traktor / Engine / VirtualDJ / djay | Same as above for each platform | IO | Per-platform adapters (Serato crates + GEOB tags, Traktor NML, Engine SQLite, VirtualDJ XML, djay) | none | Reverse-engineered formats need fidelity tests. Engine DB writes are risky. | P1 | PARTIALLY VERIFIED (Traktor, Serato, VirtualDJ) | `tests/dj_apps.rs`: whole library with folders → Rekordbox XML, Traktor NML, Serato crates, VirtualDJ .vdjfolder → re-import into a fresh library keeps tracks, playlists, BPM, key, rating, cues/loops where the format has them | Engine DJ and djay: not built (Engine writes its own SQLite database, which we do not modify; djay has no documented exchange format). Not yet loaded into the real applications. |
| Lexicon | Free | Pioneer USB conversion | Prepare a USB stick for CDJs | IO | Device library (PDB/OneLibrary) writer | possibly rekordcrate (MIT) for reading | Format is undocumented. Must not be promised before verification. | P2 | OUT OF SCOPE — JUSTIFIED | CDJ loads the stick (hardware test) | Undocumented binary device formats (PDB / OneLibrary); a wrong write can corrupt a stick used live. Supported path: export Rekordbox XML → Rekordbox → USB. |
| Lexicon | Free | Key conversion | Show/convert key notation (Camelot, Open Key, musical) | META | Key parser | none | — | P0 | PARTIALLY VERIFIED | `keys::tests` (all 24 keys round-trip across 3 notations) | UI shows Camelot + original. Key notation written to files is a setting (original / Camelot / Open Key / musical); "Rewrite key notation in files…" writes only the key with preview, backups and read-back verification (`tests/key_notation.rs`). |
| Lexicon | Free | Smart playlist conversion | Smart playlists survive conversion | CRATE/IO | Map rules to target formats | none | Each target has its own rule model | P2 | NOT STARTED | Per-platform test | Smart crates export as static snapshots (Rekordbox/Traktor/Serato/VirtualDJ) under a "Smart Crates" folder. |
| Lexicon | Free | Streaming-service playlist import (Spotify/Beatport/Tidal) | Match a streaming playlist to local files | IO | Fuzzy matcher | Service APIs/exports | API terms; needs network | P2 | PARTIALLY VERIFIED (offline) | `tests/playlist_match.rs`, `playlist_match::tests` | Offline: playlists exported to CSV (e.g. Exportify), M3U or text tracklists are matched to local files; only confident matches are pre-selected, other versions are offered for review, missing tracks listed. Direct streaming-API access is out of scope (online, account, API terms). |
| Lexicon | Free | Field mappings | Choose which field maps where on export | IO | Mapping config | none | — | P1 | NOT STARTED | Mapping persisted + applied in export test | |
| Lexicon | Free | Track timeline / play queue / waveform preview | Preview tracks with waveform and queue | UI | Audio streaming, waveform | none | — | P0 | PARTIALLY VERIFIED | `audio_protocol::serve_tests` (byte ranges, WAV fallback); UI screenshot | Audible playback not verified in the headless CI container (no sound device). |
| Lexicon | Free | DIRECT2CDJ | Play to CDJs over network | — | Pro DJ Link | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Live performance tooling (excluded by charter). |
| Lexicon | Free | Last.fm scrobbling | Report plays online | — | — | Last.fm API | needs an account | — | OUT OF SCOPE — JUSTIFIED | — | Not library management; contradicts offline/no-account principle. |
| Lexicon | Essential | Track & playlist editing, editor | Edit metadata singly and in bulk | META/UI | Provenance-aware field store | none | — | P0 | PARTIALLY VERIFIED | `import_decide_search_export_restart_analyze` (edit wins over analysis, survives re-analysis, revert works) | Writing tags into audio files is a separate, explicit action (see "Write tags to file"). |
| Lexicon | Essential | Custom tags | Free-form tags per track | META | Hierarchical tags, synonyms, categories, Quick Tag slots | none | — | P1 | PARTIALLY VERIFIED | `phase2::hierarchical_tags_synonyms_and_quick_slots` | `tag:x` matches children + synonyms. |
| Lexicon | Essential | Recipes (automation) | Saved multi-step operations | META | Recipe engine + preview | none | — | P2 | PARTIALLY VERIFIED | `tests/recipes.rs` (on demand, on import, undo) | Saved search + actions (set field, add/remove tag, decision, add to playlist, analyze); one undoable step per run; optional run on newly imported tracks (watch folders included). |
| Lexicon | Essential | Smartlists + smartlist generator | Rule-based dynamic playlists | CRATE | Query language → SQL | none | — | P0 | PARTIALLY VERIFIED | `query::tests`, smart playlist assertions in `library_flow` | No natural-language generator: building lists by sound is done with Find similar and Build a set from a track. |
| Lexicon | Essential | Archive | Hide tracks without deleting | CORE | Archive flag | none | — | P1 | PARTIALLY VERIFIED | `tests/dedupe.rs` (archived hidden from search/stats, `archived:yes`) | Undoable Archive/Restore in the Library context menu. |
| Lexicon | Essential | Incoming tracks | Inbox of newly added tracks | CORE | `added:` filter | none | — | P0 | PARTIALLY VERIFIED | `added:7d` query | No dedicated inbox view. |
| Lexicon | Essential | Energy / danceability / popularity / happiness fields | Extra descriptive fields | AN/DNA | Measured energy curve today; models later | — | — | P1 | IN DESIGN | | Popularity needs online data → out of the offline core. |
| Lexicon | Essential | Write tags to file | Persist edits into ID3/Vorbis/MP4 tags | META | Dry-run plan, changed-on-disk guard, original copy, read-back verification, auto-restore | lofty (MIT/Apache) | Unknown frames are kept by lofty (not byte-verified yet) | P1 | PARTIALLY VERIFIED | `phase2::write_tags_back_to_files` (MP3, FLAC, M4A, AIFF; audio still decodes) | ID3/MP4 store integer BPM only. Rating/POPM not written yet. |
| Lexicon | Essential | Album art add/remove | Manage artwork | META | Picture read/write | lofty | — | P2 | PARTIALLY VERIFIED | `phase2::album_art_add_and_remove_safely` (FLAC + MP3 with backup, audio still decodes, re-import unchanged, non-image refused) | Front cover from JPEG/PNG/GIF/BMP ≤ 10 MB; Library context menu. |
| Lexicon | Essential | Undo history | Undo edits | CORE | Change batches with before/after state, undo/redo stack | none | — | P1 | PARTIALLY VERIFIED | `phase2::undo_redo_edits_decisions_notes_favorites` | Covers edits, decisions, notes, favourites, tags, smart fixes. Also playlist order, file moves/relocation, archive, beatgrid edits, recipes and merges. Not yet: cue edits (imports have backups). |
| Lexicon | Essential | Set cue points/loops/beatgrid | Edit cues and grid | AN/UI | Cue table (locked user cues), grid table | none | — | P1 | PARTIALLY VERIFIED | `phase2::analysis_creates_phrase_cue_suggestions…`, `phase2::user_beatgrid_edit_loop_and_undo` | Cues at playhead, loops of 4/8/16 beats snapped to the grid, beatgrid editor (downbeat at playhead, nudge, BPM ×2/÷2, tap tempo, zoomed view); user grid wins on export; undoable. |
| Lexicon | Essential | Analyze BPM/beatgrid/key/energy | Local analysis | AN | DSP pipeline | rustfft, symphonia | MPL-2.0/MIT/Apache | P0 | PARTIALLY VERIFIED | `analysis_synthetic` (8 tempi 90–174 BPM, 8 keys, exact), multi-format test | Accuracy on real music not yet measured (see `AI_VALIDATION.md`). |
| Lexicon | Essential | Find lost, broken and unused files | Detect missing/unused | HEALTH | Existence scan, relink by hash | none | — | P0 | PARTIALLY VERIFIED | Move test (relink by hash), `tests/dedupe.rs` (broken playlists, unused files) | Library Health lists missing files, playlists with missing files and audio files in a folder that are not in the library. |
| Lexicon | Essential | Smart Fixes & cleanup (genre, artist, casing, remixer…) | Batch corrections with preview | HEALTH/META | 10 rules, per-proposal accept, one undoable batch | none | — | P1 | PARTIALLY VERIFIED | `fixes::tests`, `phase2::smart_fixes_preview_apply_undo` | Genre rule only fixes spelling of the same genre, never merges genres. |
| Lexicon | Essential | Merge/prefix/cross-reference/shuffle playlists, playlist occurrence | Playlist utilities | CORE | Playlist ops | none | — | P1 | PARTIALLY VERIFIED | `tests/bulk_tools.rs::playlist_combine_shuffle_prefix_occurrences` | Merge/common/subtract into a new playlist, duplicate, undoable shuffle, prefixes, playlists per track. |
| Lexicon | Essential | Export CSV / M3U8 / HTML/PDF | Export lists | IO | Writers | csv | — | P0 | PARTIALLY VERIFIED | M3U8 content asserted, CSV written | HTML/PDF not built. |
| Lexicon | Essential | Mobile app, Local API, plugins, Stream Deck | Remote/extension access | — | — | — | — | P3 | OUT OF SCOPE — JUSTIFIED |  | Desktop-only, offline local version: no network service is opened. The CLI (`deepcuts`) covers scripting. |
| Lexicon | Ultimate | Watch folder | Auto-import new files | CORE | OS watcher (notify) + 3 s debounce + startup rescan; removed paths marked missing → relink | notify (CC0-1.0) | — | P1 | PARTIALLY VERIFIED | Registry test + manual run on Linux: new file imported and queued for analysis, rename relinked to the same track, deletion marked missing | No automated test of the OS event loop; Windows/macOS not checked by hand. |
| Lexicon | Ultimate | Move & rename files, target folder | Organise files on disk | CORE | Safe move with undo + DB relink | none | High risk: preview + backup | P1 | PARTIALLY VERIFIED | `tests/fileops.rs` (rename, move to folder, conflicts, undo/redo, re-import consistent) + GUI check | Pattern-based; no automatic folder structure by genre/label yet. |
| Lexicon | Ultimate | History | Change history per track | CORE | `changes` + `audit_log` | none | — | P1 | PARTIALLY VERIFIED | `history::track_history` in undo test; UI in details panel | |
| Lexicon | Ultimate | Import tags from CSV | Bulk metadata from CSV | IO | CSV mapper | csv | — | P1 | PARTIALLY VERIFIED | `tests/csv_relocate.rs::csv_import_by_artist_title_and_by_path_round_trip` | Column → field mapping, match by path / ISRC / artist + title, value validation, preview, one undoable batch; empty cells never clear values. |
| Lexicon | Ultimate | Local path mappings | Same library on different drives/OS | CORE | Path prefix map | none | — | P1 | PARTIALLY VERIFIED | `tests/csv_relocate.rs::relocate_rewrites_paths_and_undoes`, `relocate::tests` | Rewrites a path prefix in the database only (drive letter, mount point, other OS), never colliding with known files; undoable. |
| Lexicon | Ultimate | Cue point templates, custom anchors, generate cue points | Automatic cues | AN | Phrase-aligned (8-bar) suggestions from beat grid + energy curve: Intro, Main, Breakdown, Drop, Outro | — | — | P1 | PARTIALLY VERIFIED (generation) | `cues::tests`, synthetic breakdown found within one beat | Suggestions only, never exported until accepted. **Cue templates**: hot/memory cues and loops anchored to first beat, main, n-th breakdown/drop, outro or end with bar offsets, snapped to the grid, one undoable step (`tests/cue_templates.rs`, GUI check). Section detection accuracy on real music not measured. |
| Lexicon | Ultimate | Automatic emergency loop | Performance safety loop | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Live performance feature. |
| Lexicon | Ultimate | Find duplicates | Duplicate detection | HEALTH | Hash + metadata groups + acoustic fingerprint groups | — | — | P0 | PARTIALLY VERIFIED | `duplicate_candidates`, `tests/fingerprint.rs`, `tests/dedupe.rs` (merge) | Merge keeps one copy, re-points playlists, carries decisions/tags/ratings, moves other files to a dated trash folder (never deleted), undoable. |
| Lexicon | Ultimate | Find missing tags & album art | Gap report | HEALTH | Stats queries | none | — | P0 | PARTIALLY VERIFIED | Dashboard "Gaps to fill" (genre:none, bpm:none, key:none, artwork:no) | |
| Lexicon | Ultimate | Discover new tracks, mixable tracks | Suggestions | SIM | Similar tracks + next-track suggestions | — | — | P1 | PARTIALLY VERIFIED | phase3 tests | Store links / online discovery out of the offline core. |
| Lexicon | Ultimate | Cloud storage & DB backup | Cloud copy of library | BAK | User-owned storage only | — | No proprietary cloud | P2 | OUT OF SCOPE — JUSTIFIED (cloud) | `backup_and_restore` | No cloud service. Local backups (manual, daily, before risky operations) are PARTIALLY VERIFIED; the backup folder can be placed in any user-owned synced folder. |
| Lexicon | Ultimate | Send playlists to Spotify/Tidal/Beatport | Push to streaming | — | — | Service APIs | — | — | OUT OF SCOPE — JUSTIFIED | — | Online accounts; not local library management. |

## DJOID — Djoid Pro (official site)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| DJOID | Pro | Chapter Builder | Split a set into 3–20-track energy blocks; export chapters as crates | SET | Chapters with roles (Opening → Closing) and energy targets | none | — | P1 | PARTIALLY VERIFIED | `phase3::set_builder_chapters_locks_sort_suggest_and_undo` | Chapters are not exported as separate crates yet (the whole set exports). Energy is a measured proxy. |
| DJOID | Pro | Graph Playlists | Graph of tracks by harmony/energy/emotion; suggests next tracks; resurfaces old tracks | SIM/UI | Beam search over the transition graph (harmony, tempo, energy target, timbre) from a seed track | — | — | P1 | PARTIALLY VERIFIED | `phase3::graph_path_from_seed…`, GUI check | Library › "Build a set from this track…" and next-track suggestions in the Set Builder. A visual graph view is not built; "emotion" is not modelled. |
| DJOID | Pro | Scatter Map | 2-D map of the whole library; proximity = compatibility | SIM/UI | PCA of measured feature vectors, canvas with pan/zoom, area selection → playlist | — | — | P1 | PARTIALLY VERIFIED | `phase3::discovery_map_projects_all_analysed_tracks`, `map::tests`, GUI check | Measured features only (no embeddings yet), so the map reflects loudness/density/dynamics more than genre. |
| DJOID | Pro | Magic Sorting | Automatic set ordering | SET | Greedy + local search (swap/relocate) on transition cost + energy target; pinned tracks fixed; undoable | none | — | P1 | PARTIALLY VERIFIED | `sets::tests` (build mode Spearman ρ > 0.9, pins kept), phase3 test | Cost model is hand-designed, not learned. |
| DJOID | Pro | Auto Group | Not described by vendor (assumed clustering) | SIM | Deterministic k-means on measured features with plain-language traits | — | — | P2 | PARTIALLY VERIFIED | `tests/dna.rs`, GUI check | Music DNA › Groups; a group can be saved as a playlist. Vendor behaviour not documented, so equivalence is assumed. |
| DJOID | Pro | Cue Points | Not described (assumed automatic cues) | AN | Phrase-aligned suggestions | — | — | P1 | PARTIALLY VERIFIED | see Lexicon "generate cue points" | |
| DJOID | Pro | Genre Detection | Automatic genre/sub-genre | DNA | Classifier + validation | model weights | Licence of weights | P1 | PARTIALLY VERIFIED (from the user's own labels) | `tests/dna.rs` (leave-one-out accuracy reported) | Genre suggestions from the nearest analysed neighbours in the user's own library, with the measured accuracy shown; no pretrained genre model yet. |
| DJOID | Pro | Matching Tracks | Compatible-track finder | SIM | Weighted, explainable similarity over measured features (tempo, harmony, energy, timbre, rhythm density, dynamics) | — | — | P0/P1 | PARTIALLY VERIFIED | `similarity::tests`, `phase3::similar_tracks_are_ranked_and_explained` | No timbral embeddings yet. |
| DJOID | Pro | Migration from DJ software (incl. Rekordbox tags) | Import playlists, crates, tags | IO | Adapters | — | — | P0 | PARTIALLY VERIFIED (Rekordbox XML, Traktor NML, Serato crates, VirtualDJ) | `rekordbox_xml_round_trip`, `tests/dj_apps.rs` | Rekordbox "My Tag" lives in master.db (encrypted), not in XML: not read. |
| DJOID | Pro | Clean export to DJ setup | Export crates | IO | Writers | — | — | P0 | PARTIALLY VERIFIED | M3U8/XML tests | |

## MIXO — MIXO:GOLD (official pricing page)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| MIXO | Free | Import from nearly all DJ apps | Read other libraries | IO | Adapters | — | — | P0/P1 | PARTIALLY VERIFIED (Rekordbox, Traktor, Serato, VirtualDJ, folders) | `tests/dj_apps.rs` | Engine DJ and djay not read. |
| MIXO | Free | Edit metadata, cues, ratings | Editing | META | Field store | — | — | P0 | PARTIALLY VERIFIED |  | Cue, loop and beatgrid editing in the details panel. |
| MIXO | Free | Analyse key and BPM | Analysis | AN | DSP | — | — | P0 | PARTIALLY VERIFIED | `analysis_synthetic` | Real-corpus accuracy pending. |
| MIXO | Free | Playlists & smart lists, fast search | Organisation | CORE/CRATE | FTS5 + query language | SQLite FTS5 | public domain | P0 | PARTIALLY VERIFIED | Bench: 100k tracks, filtered first page 0.3–120 ms | |
| MIXO | Free | Related Track Finder | Similar tracks | SIM | Explainable similarity (measured features) | — | — | P1 | PARTIALLY VERIFIED | phase3 similarity test | |
| MIXO | Gold | Sync library to all devices | Multi-device | BAK | User-owned storage / LAN sync | — | No proprietary cloud | P2 | OUT OF SCOPE — JUSTIFIED | Conflict tests | No sync service in the local version; library paths can be relocated between machines (Local path mappings). |
| MIXO | Gold | Export full library to most DJ apps | Conversion | IO | Adapters | — | — | P1 | PARTIALLY VERIFIED (Rekordbox, Traktor, Serato, VirtualDJ) | `tests/dj_apps.rs` | Engine DJ not built (see Lexicon conversion). |
| MIXO | Gold | OneLibrary / Device Library / Engine USB export | USB prep | IO | Undocumented binary formats | — | Risky | P2 | OUT OF SCOPE — JUSTIFIED | Hardware test | See Lexicon "Pioneer USB conversion". |
| MIXO | Gold | Playlist Matcher (Spotify/Tidal/Apple) | Match external playlists | IO | Fuzzy matcher | Online | — | P2 | PARTIALLY VERIFIED (offline) | `tests/playlist_match.rs` | See Lexicon streaming-playlist import. |
| MIXO | Gold | Duplicate Finder | Duplicates | HEALTH | — | — | — | P0 | PARTIALLY VERIFIED | `duplicate_candidates` | |
| MIXO | Gold | Key & cue conversion | Notation and cue mapping | META/IO | Key parser; cue mapping | — | — | P0/P1 | PARTIALLY VERIFIED (key, Rekordbox cues) | | |
| MIXO | Gold | Find and replace | Bulk text replace | META | Preview + undo | — | — | P1 | PARTIALLY VERIFIED | `tests/bulk_tools.rs::find_replace_preview_apply_undo` | Plain, whole word or regex with capture groups; preview; one undoable batch. |
| MIXO | Gold | Mobile apps | Phone/tablet | — | — | — | — | P3 | OUT OF SCOPE — JUSTIFIED |  | Desktop-only local version. |

## Mixed In Key (official site; preparation/analysis features only)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Mixed In Key | MIK | Key & BPM detection | Analyse files | AN | DSP / later model | — | — | P0 | PARTIALLY VERIFIED | `analysis_synthetic`, GiantSteps evaluation (`AI_VALIDATION.md`) | GiantSteps Key MIREX 67.5 % (exact 59.3 %, dc-dsp 1.3.0), tempo Acc1 65.8 % / Acc2 88.2 %. Below commercial tools' published claims; improving it is the next priority. |
| Mixed In Key | MIK | Camelot wheel / compatible keys | Harmonic suggestions | META/CRATE | Camelot logic | — | — | P0 | PARTIALLY VERIFIED | `keys::tests::compatibility`, `key:8A+` | Energy moves: +2 (energy boost) and +7 (semitone up) are labelled in transitions and suggestions; `key:8A++` finds compatible keys plus those moves (`query::tests::harmonic_key_queries`). |
| Mixed In Key | MIK | Energy Level (1–10) | Danceability/energy rating | DNA | Model or calibrated DSP | — | — | P1 | PARTIALLY VERIFIED (relative) | `tests/dna.rs` | 1–10 is the track's rank in the user's library on the measured energy proxy (not an absolute or human-calibrated score); correlation with human ratings not measured. |
| Mixed In Key | MIK | Cue point generation | Auto cues | AN | Phrase-aligned suggestions from energy changes | — | — | P1 | PARTIALLY VERIFIED (synthetic) | Boundary F-measure on real music pending | |
| Mixed In Key | MIK | Write results to tags | Tag writing | META | lofty writer (BPM, key incl. measured values) | — | — | P1 | PARTIALLY VERIFIED | `phase2::write_tags_back_to_files` | |
| Mixed In Key | Pro | Mashup / transition ideas | Mashup suggestions | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Mashups excluded. Transition *compatibility* suggestions live in SET (P1). |
| Mixed In Key | Live / Studio | Real-time/DAW key detection | — | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Production/DAW tooling. |

## One Tagger (third-party listings; open source GPL-3.0)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| One Tagger | Free | Auto tagger from Beatport/Traxsource/Juno/Discogs/MusicBrainz | Fetch metadata for matched tracks | META | Matchers per catalog | Online APIs | Each catalog's ToS/rate limits must be checked. MusicBrainz is CC0 data. | P2 | OUT OF SCOPE — JUSTIFIED | Match precision on sample set | Requires online catalogues; the local version makes no network requests. |
| One Tagger | Free | Quick Tag with shortcuts (energy, mood, genre) | Keyboard tagging while listening | UI/META | Shift+1…9 quick-tag slots (toggle on selection) | — | — | P1 | PARTIALLY VERIFIED | Core test + GUI check | |
| One Tagger | Free | Audio features (Spotify) → tags | Online features | — | Spotify API | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Remote AI/feature service conflicts with local-only analysis. Local equivalents come from DNA. |
| One Tagger | Free | Auto rename from template | Rename files | CORE | Safe rename + relink | — | High risk: preview/undo | P1 | PARTIALLY VERIFIED | `tests/fileops.rs`, `fileops::tests` | Tokens {artist} {title} {version} {remixer} {album} {label} {genre} {year} {bpm} {key} {camelot}; preview + undo. |
| One Tagger | Free | Formats MP3/AIFF/FLAC/M4A | Read/write tags | META | lofty | — | — | P0 | PARTIALLY VERIFIED | Read: `encoded_formats_with_tags`; write: `write_tags_back_to_files` | |

## Vibes (developer posts + listings)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Vibes | Licence | Tag by vibe/energy/mood/era and combine filters | Tag-centric organisation | META/CRATE | Tag categories + query language | — | — | P1 | PARTIALLY VERIFIED | Tag tests + `tag:a -tag:b` queries | |
| Vibes | Licence | On-device AI that learns your tagging and suggests tags | Personal tag suggestions | TASTE/DNA | Local classifier on embeddings | model | — | P1 | PARTIALLY VERIFIED | `tests/dna.rs` | Nearest-neighbour tag suggestions from the user's own tagging on measured features, sent to the review queue (rejections remembered). |
| Vibes | Licence | BPM/key/cue analysis on import | Auto analysis | AN | — | — | — | P0 | PARTIALLY VERIFIED (BPM/key; cues not yet) | | |
| Vibes | Licence | Set Designer canvas with key/BPM suggestions | Visual set prep | SET | Set Builder list with transition badges, energy/BPM progression, suggestions | — | — | P1 | PARTIALLY VERIFIED | phase3 test + GUI check | List-based, not a free canvas. |
| Vibes | Licence | Export to Rekordbox/Serato/Traktor/Engine; Traktor import read-only | Interop | IO | Adapters | — | — | P0/P1 | PARTIALLY VERIFIED (Rekordbox) | | |

## Music Library Doctor (AlternativeTo listing only — low confidence)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Music Library Doctor | Pro | Reads Rekordbox/Serato/VirtualDJ libraries directly | Diagnose without converting | IO/HEALTH | Readers | — | Read-only access to proprietary DBs is acceptable; writing is not | P1 | PARTIALLY VERIFIED (Serato crates, VirtualDJ database, Rekordbox/Traktor exports) | `tests/dj_apps.rs` | Rekordbox master.db is not read (encrypted). |
| Music Library Doctor | Pro | Duplicates by sound across formats/bitrates | Acoustic duplicates | HEALTH | Own band-energy fingerprint (Haitsma–Kalker scheme) | none | Own code, no LGPL | P1 | PARTIALLY VERIFIED | `tests/fingerprint.rs`: FLAC/MP3 320/MP3 128/AAC/−6 dB/trimmed copies grouped (BER ≤ 0.24), other recordings not (≥ 0.43) | False-positive rate on a large real library not measured; only tracks within ±2 s duration are compared. |
| Music Library Doctor | Pro | Re-point playlists to the kept copy before trashing | Safe dedupe | HEALTH | Merge tracks + playlist remap + trash (never delete) | — | — | P1 | PARTIALLY VERIFIED | `tests/dedupe.rs` | See Lexicon "Find duplicates". |

## Crate Hackers (review only; local-library-relevant parts)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Crate Hackers | Sub | AI suggestions by key/harmony/BPM | Next-track suggestions | SET/SIM | Transition score + energy target + sound similarity | — | — | P1 | PARTIALLY VERIFIED | `phase3` suggest_next assertions | Not "AI": a documented scoring function. |
| Crate Hackers | Sub | Magic Sorter (playlist → mixable crate) | Reorder for mixability | SET | Magic Sort | — | — | P1 | PARTIALLY VERIFIED | see DJOID Magic Sorting | |
| Crate Hackers | Sub | Pre-made crates / "Banger button" | Curated online content | — | — | Online catalog | — | — | OUT OF SCOPE — JUSTIFIED | — | Online content service, not local library management. |

## Crate Intelligence Pro — BLOCKED

Product not found under this name (2026-10-20). Status **BLOCKED** until the product owner provides an
official link. No features were assumed.

## DJ.Studio — Ultimate (selection/ordering only)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| DJ.Studio | Ultimate | Automatic playlist ordering (harmonic/energy) | Order a playlist for flow | SET | Magic Sort (smooth / build / arc, or chapter targets) | — | — | P1 | PARTIALLY VERIFIED | see DJOID Magic Sorting | DJ.Studio feature list not audited from the official source. |
| DJ.Studio | Ultimate | Timeline mixing, effects, video/audio rendering, mashups, stems | — | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Production and rendering are excluded by charter. |

---

## Cross-cutting requirements from the master prompt (not tied to one competitor)

| Area | Requirement | Status | Evidence |
|---|---|---|---|
| Data safety | Read-only import by default | PARTIALLY VERIFIED | Import code never opens audio files for writing. Tests import then re-import with identical results. |
| Data safety | User values never overwritten by imports/AI | PARTIALLY VERIFIED | Provenance precedence `user > import:* > file > analysis`, tests 7 and Rekordbox re-import |
| Data safety | Backup before risky operations | PARTIALLY VERIFIED | Pre-Rekordbox-import and pre-restore snapshots, daily auto backup (app) |
| Data safety | File identity independent of path | PARTIALLY VERIFIED | UUID track ids + content hash relink (move test) |
| Analysis accuracy | Measured on real music | PARTIALLY VERIFIED | GiantSteps: tempo Acc1 65.8 % / Acc2 88.2 %, key MIREX 67.5 % (exact 59.3 %, dc-dsp 1.3.0 with tuning estimate, dev/test protocol); exact BPM on held-out Beatport labels measured separately (`AI_VALIDATION.md`) |
| Audio quality | Transcode / fake-lossless detection | PARTIALLY VERIFIED | Measured bandwidth cliff + stated rule (lossless with cut-off ≤ 19.8 kHz; ≥ 256 kbps lossy with cut-off ≤ 17.5 kHz). `tests/transcode.rs` (FLAC and 320 kbps made from a stereo 128 kbps LAME MP3 flagged; genuine FLAC, genuine 320k, honest 128k not), `bandwidth::tests`. Health section + `transcode:yes`. Not detected: sources whose encoder kept ~20 kHz (mono 128 kbps, 256–320 kbps). |
| Performance | 100k tracks | PARTIALLY VERIFIED | `deepcuts bench 100000` (see `docs/BENCHMARKS.md`) |
| Jobs | Persistent queue, crash recovery, cancellation | PARTIALLY VERIFIED | `interrupted_jobs_are_recovered_once`, `cancellation` |
| Privacy | Offline, no account, no remote AI | VERIFIED (by construction) | No network code in the app. The only online step is the optional dependency download at build time. |
| i18n | Italian + English | PARTIALLY VERIFIED | `i18n.test.ts` (full key + placeholder parity) |
