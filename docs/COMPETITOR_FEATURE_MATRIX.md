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
| Lexicon | Free | Conversion to Serato / Traktor / Engine / VirtualDJ / djay | Same as above for each platform | IO | Per-platform adapters (Serato crates + GEOB tags, Traktor NML, Engine SQLite, VirtualDJ XML, djay) | none | Reverse-engineered formats need fidelity tests. Engine DB writes are risky. | P1 | NOT STARTED | Round-trip fidelity suite per platform | M3U8 export works with all of them today (tracks + order only). |
| Lexicon | Free | Pioneer USB conversion | Prepare a USB stick for CDJs | IO | Device library (PDB/OneLibrary) writer | possibly rekordcrate (MIT) for reading | Format is undocumented. Must not be promised before verification. | P2 | NOT STARTED | CDJ loads the stick (hardware test) | Documented path today: Rekordbox XML → Rekordbox → USB. |
| Lexicon | Free | Key conversion | Show/convert key notation (Camelot, Open Key, musical) | META | Key parser | none | — | P0 | PARTIALLY VERIFIED | `keys::tests` (all 24 keys round-trip across 3 notations) | UI shows Camelot + original. A bulk "rewrite key notation in tags" action is not built. |
| Lexicon | Free | Smart playlist conversion | Smart playlists survive conversion | CRATE/IO | Map rules to target formats | none | Each target has its own rule model | P2 | NOT STARTED | Per-platform test | Smart crates export as static lists today. |
| Lexicon | Free | Streaming-service playlist import (Spotify/Beatport/Tidal) | Match a streaming playlist to local files | IO | Fuzzy matcher | Service APIs/exports | API terms; needs network | P2 | NOT STARTED | Match-rate report on a test playlist | Would be the optional online part; the app stays usable offline. |
| Lexicon | Free | Field mappings | Choose which field maps where on export | IO | Mapping config | none | — | P1 | NOT STARTED | Mapping persisted + applied in export test | |
| Lexicon | Free | Track timeline / play queue / waveform preview | Preview tracks with waveform and queue | UI | Audio streaming, waveform | none | — | P0 | PARTIALLY VERIFIED | `audio_protocol::serve_tests` (byte ranges, WAV fallback); UI screenshot | Audible playback not verified in the headless CI container (no sound device). |
| Lexicon | Free | DIRECT2CDJ | Play to CDJs over network | — | Pro DJ Link | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Live performance tooling (excluded by charter). |
| Lexicon | Free | Last.fm scrobbling | Report plays online | — | — | Last.fm API | needs an account | — | OUT OF SCOPE — JUSTIFIED | — | Not library management; contradicts offline/no-account principle. |
| Lexicon | Essential | Track & playlist editing, editor | Edit metadata singly and in bulk | META/UI | Provenance-aware field store | none | — | P0 | PARTIALLY VERIFIED | `import_decide_search_export_restart_analyze` (edit wins over analysis, survives re-analysis, revert works) | Writing tags back into audio files is not built (read-only by design for now). |
| Lexicon | Essential | Custom tags | Free-form tags per track | META | Hierarchical tags, synonyms, categories, Quick Tag slots | none | — | P1 | PARTIALLY VERIFIED | `phase2::hierarchical_tags_synonyms_and_quick_slots` | `tag:x` matches children + synonyms. |
| Lexicon | Essential | Recipes (automation) | Saved multi-step operations | META | Recipe engine + preview | none | — | P2 | NOT STARTED | | |
| Lexicon | Essential | Smartlists + smartlist generator | Rule-based dynamic playlists | CRATE | Query language → SQL | none | — | P0 | PARTIALLY VERIFIED | `query::tests`, smart playlist assertions in `library_flow` | Generator from natural language arrives with Music DNA. |
| Lexicon | Essential | Archive | Hide tracks without deleting | CORE | Archive flag | none | — | P1 | NOT STARTED | | Decisions (`pass`) cover part of the need. |
| Lexicon | Essential | Incoming tracks | Inbox of newly added tracks | CORE | `added:` filter | none | — | P0 | PARTIALLY VERIFIED | `added:7d` query | No dedicated inbox view. |
| Lexicon | Essential | Energy / danceability / popularity / happiness fields | Extra descriptive fields | AN/DNA | Measured energy curve today; models later | — | — | P1 | IN DESIGN | | Popularity needs online data → out of the offline core. |
| Lexicon | Essential | Write tags to file | Persist edits into ID3/Vorbis/MP4 tags | META | Dry-run plan, changed-on-disk guard, original copy, read-back verification, auto-restore | lofty (MIT/Apache) | Unknown frames are kept by lofty (not byte-verified yet) | P1 | PARTIALLY VERIFIED | `phase2::write_tags_back_to_files` (MP3, FLAC, M4A, AIFF; audio still decodes) | ID3/MP4 store integer BPM only. Rating/POPM not written yet. |
| Lexicon | Essential | Album art add/remove | Manage artwork | META | Picture read/write | lofty | — | P2 | NOT STARTED | | `has_artwork` detected on import. |
| Lexicon | Essential | Undo history | Undo edits | CORE | Change batches with before/after state, undo/redo stack | none | — | P1 | PARTIALLY VERIFIED | `phase2::undo_redo_edits_decisions_notes_favorites` | Covers edits, decisions, notes, favourites, tags, smart fixes. Not yet: playlist and cue edits (imports have backups). |
| Lexicon | Essential | Set cue points/loops/beatgrid | Edit cues and grid | AN/UI | Cue table (locked user cues), grid table | none | — | P1 | PARTIALLY VERIFIED (cues) | `phase2::analysis_creates_phrase_cue_suggestions…` | Memory/hot cues at playhead (C key), rename, delete. Loop and beatgrid editing UI pending. |
| Lexicon | Essential | Analyze BPM/beatgrid/key/energy | Local analysis | AN | DSP pipeline | rustfft, symphonia | MPL-2.0/MIT/Apache | P0 | PARTIALLY VERIFIED | `analysis_synthetic` (8 tempi 90–174 BPM, 8 keys, exact), multi-format test | Accuracy on real music not yet measured (see `AI_VALIDATION.md`). |
| Lexicon | Essential | Find lost, broken and unused files | Detect missing/unused | HEALTH | Existence scan, relink by hash | none | — | P0 | PARTIALLY VERIFIED | Move test: file renamed → missing → re-import → relinked with decisions kept | "Unused files" (files on disk not in library) not built. |
| Lexicon | Essential | Smart Fixes & cleanup (genre, artist, casing, remixer…) | Batch corrections with preview | HEALTH/META | 10 rules, per-proposal accept, one undoable batch | none | — | P1 | PARTIALLY VERIFIED | `fixes::tests`, `phase2::smart_fixes_preview_apply_undo` | Genre rule only fixes spelling of the same genre, never merges genres. |
| Lexicon | Essential | Merge/prefix/cross-reference/shuffle playlists, playlist occurrence | Playlist utilities | CORE | Playlist ops | none | — | P1 | NOT STARTED | | "In playlists" is shown per track in details. |
| Lexicon | Essential | Export CSV / M3U8 / HTML/PDF | Export lists | IO | Writers | csv | — | P0 | PARTIALLY VERIFIED | M3U8 content asserted, CSV written | HTML/PDF not built. |
| Lexicon | Essential | Mobile app, Local API, plugins, Stream Deck | Remote/extension access | — | — | — | — | P3 | NOT STARTED | | Phase 8 optional (mobile companion architecture). |
| Lexicon | Ultimate | Watch folder | Auto-import new files | CORE | OS watcher (notify) + 3 s debounce + startup rescan; removed paths marked missing → relink | notify (CC0-1.0) | — | P1 | PARTIALLY VERIFIED | Registry test + manual run on Linux: new file imported and queued for analysis, rename relinked to the same track, deletion marked missing | No automated test of the OS event loop; Windows/macOS not checked by hand. |
| Lexicon | Ultimate | Move & rename files, target folder | Organise files on disk | CORE | Safe move with undo + DB relink | none | High risk: preview + backup | P1 | NOT STARTED | | |
| Lexicon | Ultimate | History | Change history per track | CORE | `changes` + `audit_log` | none | — | P1 | PARTIALLY VERIFIED | `history::track_history` in undo test; UI in details panel | |
| Lexicon | Ultimate | Import tags from CSV | Bulk metadata from CSV | IO | CSV mapper | csv | — | P1 | NOT STARTED | | |
| Lexicon | Ultimate | Local path mappings | Same library on different drives/OS | CORE | Path prefix map | none | — | P1 | NOT STARTED | | File identity is independent of path already. |
| Lexicon | Ultimate | Cue point templates, custom anchors, generate cue points | Automatic cues | AN | Phrase-aligned (8-bar) suggestions from beat grid + energy curve: Intro, Main, Breakdown, Drop, Outro | — | — | P1 | PARTIALLY VERIFIED (generation) | `cues::tests`, synthetic breakdown found within one beat | Suggestions only, never exported until accepted. Templates/anchors not built. Real-music accuracy not measured. |
| Lexicon | Ultimate | Automatic emergency loop | Performance safety loop | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Live performance feature. |
| Lexicon | Ultimate | Find duplicates | Duplicate detection | HEALTH | Hash + metadata groups; acoustic fingerprint later | — | — | P0 | PARTIALLY VERIFIED | `duplicate_candidates` (copy at another path found; nothing deleted) | Cross-format/bitrate duplicates need fingerprints (P1). |
| Lexicon | Ultimate | Find missing tags & album art | Gap report | HEALTH | Stats queries | none | — | P0 | PARTIALLY VERIFIED | Dashboard "Gaps to fill" (genre:none, bpm:none, key:none, artwork:no) | |
| Lexicon | Ultimate | Discover new tracks, mixable tracks | Suggestions | SIM | Harmonic + tempo filters now, embeddings later | — | — | P1 | IN DESIGN | `key:8A+` compatibility query tested | Store links / online discovery out of the offline core. |
| Lexicon | Ultimate | Cloud storage & DB backup | Cloud copy of library | BAK | User-owned storage only | — | No proprietary cloud | P2 | NOT STARTED | | Local backups (manual, daily auto, pre-risk) PARTIALLY VERIFIED (`backup_and_restore`). |
| Lexicon | Ultimate | Send playlists to Spotify/Tidal/Beatport | Push to streaming | — | — | Service APIs | — | — | OUT OF SCOPE — JUSTIFIED | — | Online accounts; not local library management. |

## DJOID — Djoid Pro (official site)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| DJOID | Pro | Chapter Builder | Split a set into 3–20-track energy blocks; export chapters as crates | SET | Energy descriptors, sequencing | none | — | P1 | NOT STARTED | Chapters exported as Rekordbox playlists | Energy curve per track exists. |
| DJOID | Pro | Graph Playlists | Graph of tracks by harmony/energy/emotion; suggests next tracks; resurfaces old tracks | SIM/UI | Similarity graph + layout | embedding model | Model licence check | P1 | NOT STARTED | Precision@K vs human pairs | Requires the embedding work of Phase 5. |
| DJOID | Pro | Scatter Map | 2-D map of the whole library; proximity = compatibility | SIM/UI | Embeddings + UMAP/t-SNE + WebGL/canvas | — | — | P1 | NOT STARTED | Renders 100k points fluidly | |
| DJOID | Pro | Magic Sorting | Automatic set ordering | SET | Sequencing optimiser | none | — | P1 | NOT STARTED | | |
| DJOID | Pro | Auto Group | Not described by vendor (assumed clustering) | SIM | Clustering | — | — | P2 | IN DESIGN | | Behaviour to be confirmed. |
| DJOID | Pro | Cue Points | Not described (assumed automatic cues) | AN | Phrase-aligned suggestions | — | — | P1 | PARTIALLY VERIFIED | see Lexicon "generate cue points" | |
| DJOID | Pro | Genre Detection | Automatic genre/sub-genre | DNA | Classifier + validation | model weights | Licence of weights | P1 | NOT STARTED | Macro-F1 on annotated corpus | Not faked: UI says "planned". |
| DJOID | Pro | Matching Tracks | Compatible-track finder | SIM | Harmonic/tempo now, embedding later | — | — | P0/P1 | PARTIALLY VERIFIED (harmonic) | `key:8A+ bpm:…` query | Timbral similarity not built. |
| DJOID | Pro | Migration from DJ software (incl. Rekordbox tags) | Import playlists, crates, tags | IO | Adapters | — | — | P0 | PARTIALLY VERIFIED (Rekordbox XML) | `rekordbox_xml_round_trip` | Rekordbox "My Tag" data lives in master.db, not in XML: needs a separate, safe read path. |
| DJOID | Pro | Clean export to DJ setup | Export crates | IO | Writers | — | — | P0 | PARTIALLY VERIFIED | M3U8/XML tests | |

## MIXO — MIXO:GOLD (official pricing page)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| MIXO | Free | Import from nearly all DJ apps | Read other libraries | IO | Adapters | — | — | P0/P1 | PARTIALLY VERIFIED (Rekordbox XML, folders, M3U8 export) | | Other platforms: NOT STARTED. |
| MIXO | Free | Edit metadata, cues, ratings | Editing | META | Field store | — | — | P0 | PARTIALLY VERIFIED | | Cue editing UI pending. |
| MIXO | Free | Analyse key and BPM | Analysis | AN | DSP | — | — | P0 | PARTIALLY VERIFIED | `analysis_synthetic` | Real-corpus accuracy pending. |
| MIXO | Free | Playlists & smart lists, fast search | Organisation | CORE/CRATE | FTS5 + query language | SQLite FTS5 | public domain | P0 | PARTIALLY VERIFIED | Bench: 100k tracks, filtered first page 0.3–120 ms | |
| MIXO | Free | Related Track Finder | Similar tracks | SIM | — | — | — | P1 | IN DESIGN (harmonic only) | | |
| MIXO | Gold | Sync library to all devices | Multi-device | BAK | User-owned storage / LAN sync | — | No proprietary cloud | P2 | NOT STARTED | Conflict tests | |
| MIXO | Gold | Export full library to most DJ apps | Conversion | IO | Adapters | — | — | P1 | NOT STARTED (except Rekordbox XML) | | |
| MIXO | Gold | OneLibrary / Device Library / Engine USB export | USB prep | IO | Undocumented binary formats | — | Risky | P2 | NOT STARTED | Hardware test | Won't be claimed until verified on hardware. |
| MIXO | Gold | Playlist Matcher (Spotify/Tidal/Apple) | Match external playlists | IO | Fuzzy matcher | Online | — | P2 | NOT STARTED | | |
| MIXO | Gold | Duplicate Finder | Duplicates | HEALTH | — | — | — | P0 | PARTIALLY VERIFIED | `duplicate_candidates` | |
| MIXO | Gold | Key & cue conversion | Notation and cue mapping | META/IO | Key parser; cue mapping | — | — | P0/P1 | PARTIALLY VERIFIED (key, Rekordbox cues) | | |
| MIXO | Gold | Find and replace | Bulk text replace | META | Preview + undo | — | — | P1 | NOT STARTED | | Bulk "set field" exists. |
| MIXO | Gold | Mobile apps | Phone/tablet | — | — | — | — | P3 | NOT STARTED | | |

## Mixed In Key (official site; preparation/analysis features only)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Mixed In Key | MIK | Key & BPM detection | Analyse files | AN | DSP / later model | — | — | P0 | PARTIALLY VERIFIED | `analysis_synthetic` | Key on real music: Krumhansl profiles are a baseline; expect weaker accuracy on modal/atonal tracks until evaluated. |
| Mixed In Key | MIK | Camelot wheel / compatible keys | Harmonic suggestions | META/CRATE | Camelot logic | — | — | P0 | PARTIALLY VERIFIED | `keys::tests::compatibility`, `key:8A+` | Energy-boost moves (+2, +7) not built. |
| Mixed In Key | MIK | Energy Level (1–10) | Danceability/energy rating | DNA | Model or calibrated DSP | — | — | P1 | NOT STARTED | Correlation with human ratings | A measured energy curve exists; no 1–10 score is shown (would be invented). |
| Mixed In Key | MIK | Cue point generation | Auto cues | AN | Phrase-aligned suggestions from energy changes | — | — | P1 | PARTIALLY VERIFIED (synthetic) | Boundary F-measure on real music pending | |
| Mixed In Key | MIK | Write results to tags | Tag writing | META | lofty writer (BPM, key incl. measured values) | — | — | P1 | PARTIALLY VERIFIED | `phase2::write_tags_back_to_files` | |
| Mixed In Key | Pro | Mashup / transition ideas | Mashup suggestions | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Mashups excluded. Transition *compatibility* suggestions live in SET (P1). |
| Mixed In Key | Live / Studio | Real-time/DAW key detection | — | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Production/DAW tooling. |

## One Tagger (third-party listings; open source GPL-3.0)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| One Tagger | Free | Auto tagger from Beatport/Traxsource/Juno/Discogs/MusicBrainz | Fetch metadata for matched tracks | META | Matchers per catalog | Online APIs | Each catalog's ToS/rate limits must be checked. MusicBrainz is CC0 data. | P2 | NOT STARTED | Match precision on sample set | Optional and online. The core works without it. |
| One Tagger | Free | Quick Tag with shortcuts (energy, mood, genre) | Keyboard tagging while listening | UI/META | Shift+1…9 quick-tag slots (toggle on selection) | — | — | P1 | PARTIALLY VERIFIED | Core test + GUI check | |
| One Tagger | Free | Audio features (Spotify) → tags | Online features | — | Spotify API | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Remote AI/feature service conflicts with local-only analysis. Local equivalents come from DNA. |
| One Tagger | Free | Auto rename from template | Rename files | CORE | Safe rename + relink | — | High risk: preview/undo | P1 | NOT STARTED | | |
| One Tagger | Free | Formats MP3/AIFF/FLAC/M4A | Read/write tags | META | lofty | — | — | P0 | PARTIALLY VERIFIED | Read: `encoded_formats_with_tags`; write: `write_tags_back_to_files` | |

## Vibes (developer posts + listings)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Vibes | Licence | Tag by vibe/energy/mood/era and combine filters | Tag-centric organisation | META/CRATE | Tag categories + query language | — | — | P1 | PARTIALLY VERIFIED | Tag tests + `tag:a -tag:b` queries | |
| Vibes | Licence | On-device AI that learns your tagging and suggests tags | Personal tag suggestions | TASTE/DNA | Local classifier on embeddings | model | — | P1 | NOT STARTED | Suggestion precision vs user's later choices | |
| Vibes | Licence | BPM/key/cue analysis on import | Auto analysis | AN | — | — | — | P0 | PARTIALLY VERIFIED (BPM/key; cues not yet) | | |
| Vibes | Licence | Set Designer canvas with key/BPM suggestions | Visual set prep | SET | Canvas + compatibility | — | — | P1 | NOT STARTED | | |
| Vibes | Licence | Export to Rekordbox/Serato/Traktor/Engine; Traktor import read-only | Interop | IO | Adapters | — | — | P0/P1 | PARTIALLY VERIFIED (Rekordbox) | | |

## Music Library Doctor (AlternativeTo listing only — low confidence)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Music Library Doctor | Pro | Reads Rekordbox/Serato/VirtualDJ libraries directly | Diagnose without converting | IO/HEALTH | Readers | — | Read-only access to proprietary DBs is acceptable; writing is not | P1 | NOT STARTED | | |
| Music Library Doctor | Pro | Duplicates by sound across formats/bitrates | Acoustic duplicates | HEALTH | Chromaprint-like fingerprint | Chromaprint (LGPL-2.1) or own | LGPL linking terms | P1 | NOT STARTED | False-positive rate on a curated set | |
| Music Library Doctor | Pro | Re-point playlists to the kept copy before trashing | Safe dedupe | HEALTH | Merge tracks + playlist remap + trash (never delete) | — | — | P1 | NOT STARTED | Playlist integrity test | |

## Crate Hackers (review only; local-library-relevant parts)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Crate Hackers | Sub | AI suggestions by key/harmony/BPM | Next-track suggestions | SET/SIM | Compatibility scoring | — | — | P1 | IN DESIGN | | Harmonic filter exists. |
| Crate Hackers | Sub | Magic Sorter (playlist → mixable crate) | Reorder for mixability | SET | Sequencing | — | — | P1 | NOT STARTED | | |
| Crate Hackers | Sub | Pre-made crates / "Banger button" | Curated online content | — | — | Online catalog | — | — | OUT OF SCOPE — JUSTIFIED | — | Online content service, not local library management. |

## Crate Intelligence Pro — BLOCKED

Product not found under this name (2026-10-20). Status **BLOCKED** until the product owner provides an
official link. No features were assumed.

## DJ.Studio — Ultimate (selection/ordering only)

| Competitor | Plan | Feature | Expected workflow / behaviour | Module | Technical requirements | External deps | Compatibility & licensing | Prio | Status | Acceptance test | Limitations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| DJ.Studio | Ultimate | Automatic playlist ordering (harmonic/energy) | Order a playlist for flow | SET | Optimiser | — | — | P1 | NOT STARTED | | Feature list not audited from official source yet. |
| DJ.Studio | Ultimate | Timeline mixing, effects, video/audio rendering, mashups, stems | — | — | — | — | — | — | OUT OF SCOPE — JUSTIFIED | — | Production and rendering are excluded by charter. |

---

## Cross-cutting requirements from the master prompt (not tied to one competitor)

| Area | Requirement | Status | Evidence |
|---|---|---|---|
| Data safety | Read-only import by default | PARTIALLY VERIFIED | Import code never opens audio files for writing. Tests import then re-import with identical results. |
| Data safety | User values never overwritten by imports/AI | PARTIALLY VERIFIED | Provenance precedence `user > import:* > file > analysis`, tests 7 and Rekordbox re-import |
| Data safety | Backup before risky operations | PARTIALLY VERIFIED | Pre-Rekordbox-import and pre-restore snapshots, daily auto backup (app) |
| Data safety | File identity independent of path | PARTIALLY VERIFIED | UUID track ids + content hash relink (move test) |
| Performance | 100k tracks | PARTIALLY VERIFIED | `deepcuts bench 100000` (see `docs/BENCHMARKS.md`) |
| Jobs | Persistent queue, crash recovery, cancellation | PARTIALLY VERIFIED | `interrupted_jobs_are_recovered_once`, `cancellation` |
| Privacy | Offline, no account, no remote AI | VERIFIED (by construction) | No network code in the app. The only online step is the optional dependency download at build time. |
| i18n | Italian + English | PARTIALLY VERIFIED | `i18n.test.ts` (full key + placeholder parity) |
