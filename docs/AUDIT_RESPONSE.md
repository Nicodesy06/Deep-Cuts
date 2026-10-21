# Response to the independent technical audit (21 October 2026)

The audit *“Deep Cuts — Audit tecnico e piano di correzione prioritario”* listed 48 items
(7 P0, 33 P1, 8 P2). This page records, for each one, what was changed, how it is tested, and what
is still open and why. Status words:

* **Fixed** — the risk described is removed in code and covered by an automated test.
* **Mitigated** — the risk is reduced in code or UI; part of the acceptance criterion needs
  resources this project does not have yet (named in the row).
* **Documented** — a limit of the method, stated where users see it; no code change can remove it.
* **Open** — not done yet; the reason and what is needed are given.

Fixes are in v0.2.1. Tests named below run in CI on Windows, macOS and Linux.

## Phase 0 — data integrity and release

| ID | Status | What was done | Evidence |
|---|---|---|---|
| DC-01 | **Fixed** | Every file move writes a synced journal entry next to the database before touching the disk. The SQL transaction's outcome settles it: commit drops the entry, failure moves the files back (newest first). Opening the library reconciles leftover entries with the committed database. | `tests/file_recovery.rs`: `rollback_reverses_moves`, `crash_before_commit_is_recovered_on_open` |
| DC-02 | **Fixed** | A missing source counts as “already moved” only if the target holds exactly the registered file (size + hash); otherwise the move is refused and the database is untouched. | `missing_source_with_different_target_is_refused`, `missing_source_with_same_file_at_target_resumes` |
| DC-03 | **Fixed** | Cross-volume moves copy to `.deepcuts-partial.*`, sync, compare the full BLAKE3 hash, rename atomically, sync the folder, then remove the source. Recovery removes abandoned partial copies. Same-volume moves use a no-overwrite hard link. | `moved_file_is_byte_identical`, `crash_before_commit_is_recovered_on_open` |
| DC-04 | **Fixed** | Duplicate merge is all-or-nothing: one failed move cancels the whole merge (nothing archived, no playlist re-pointed, moved files put back, no undo entry). | `merge_is_all_or_nothing` |
| DC-05 | **Mitigated** | Merging is never automatic. For groups found by sound or by artist + title the merge button stays disabled until the user confirms having listened (edits, remixes, live versions and shared loops are named). Open: a measured false-positive rate on a manually verified set of hundreds of pairs. | `MergeDialog.tsx` |
| DC-13 | **Open** | Export/import to Rekordbox, Traktor, Serato and VirtualDJ is round-trip tested against each format (`tests/dj_apps.rs`), but has not yet been checked inside each application and version. The website and README now say so. Needed: the applications on Windows and macOS and the field-by-field checklist in `TEST_PLAN.md`. | site, README, this page |
| DC-39 | **Fixed** | v0.2.0 was published from the commit that carries version 0.2.0 (installers for macOS, Windows and Linux); the website reads the latest release from GitHub, so the download buttons always match it. Every release now carries `SHA256SUMS.txt`. | GitHub release v0.2.0, `release.yml` |

## Phase 1 — regressions and interoperability

| ID | Status | What was done | Evidence |
|---|---|---|---|
| DC-06 | **Fixed** | Tags are written to a staged copy next to the file, read back and verified, and only then replace the original (atomic rename). A failed write never touches the original, with or without a backup folder. | `failed_tag_write_keeps_original_bytes` |
| DC-07 | **Fixed** | Before the original is replaced, a journal entry records that the library must be refreshed from the file; a failed refresh or a crash is reconciled when the library opens, and the error message says so. | `failed_refresh_is_reconciled_on_open` |
| DC-08 | **Fixed** | Libraries created at every schema version v1…v11 are upgraded to the current one in CI with tracks, files, decisions, tags, playlists and ids compared before/after, integrity and foreign-key checks. A copy of the old database (`lib.db.pre-vN.bak`) is kept before any upgrade; a newer schema is refused. | `tests/migrations.rs` |
| DC-09 | **Mitigated** | Generated file names avoid Windows device names (CON, NUL, COM1…), are shortened at a character boundary, and plans whose full path is too long for Windows are skipped with a reason. Open: Unicode normalisation differences and a test matrix on exFAT/NTFS volumes. | `fileops` unit tests |
| DC-10 | **Fixed** | A vanished watched path (file or folder) marks every file under it missing, matching whole path components only; re-importing relinks them with their decisions. | `renamed_folder_marks_children_and_relinks` (300 files) |
| DC-11 | **Open** | Watcher events are not yet tested end-to-end on all three systems (only the reconciliation logic is). Needed: an OS-level event test harness in CI. | — |
| DC-12 | **Documented** | Backups cover the library database (said in Settings). Audio files are protected differently: merges move copies to a trash folder, tag writes can keep copies of the originals. A combined “catalogue + files” backup is open. | Settings › Backups |
| DC-14 | **Documented** | Smart Crates are exported as playlists of their current tracks; the export panel says why (formats without documented smart lists). | `mapping.smartHint` |
| DC-15 | **Documented** | My Tags are read from comments; returning them to Rekordbox goes through one playlist per My Tag, as explained in the app and guide. | Sync & Export, guide |
| DC-16 | **Open** | Cue/grid conversions are unit-tested per format; tolerances inside the real applications are not measured yet (needs DC-13). | — |
| DC-17 | **Mitigated** | The auto-tagger needs artist, title, version and similar duration (or identical ISRC), shows every proposed field for review, writes only selected fields and is undoable. Open: a measured set of 100+ edit/remix homonyms. | `tests/autotag_apply.rs` |
| DC-36 | **Open** | A desktop smoke test runs the real installers on three systems (launch, analysis, model, playback). A full end-to-end UI suite (tauri-driver) is not built yet. | `scripts/installer_smoke.py` |
| DC-37 | **Fixed** | Regression tests: every setting the UI writes is accepted by the backend; FLAC and OGG comments are read on import and after re-import. | `settings_tests`, `flac_and_ogg_comments_are_read` |
| DC-38 | **Open** | Playback is checked in CI with a null audio sink; listening tests on real devices are a manual checklist (`TEST_PLAN.md`). | — |

## Phase 2 — musical quality

These are limits of current methods. Each one is measured and published in
[AI_VALIDATION.md](AI_VALIDATION.md); none of them is applied to the library automatically.

| ID | Status | Notes |
|---|---|---|
| DC-18 | **Documented** | Key: 59.3 % exact, MIREX 67.5 % on GiantSteps. A key you set is never overwritten by analysis. Per-section key and a confidence flag are open. |
| DC-19 | **Documented** | Tempo: reported separately for Beatport labels (Acc1 92.5 %) and GiantSteps perceptual labels (Acc1 65.8 %); the BPM range setting chooses the octave. Grid drift with annotated downbeats is open (no annotated corpus). |
| DC-20 | **Documented** | Cue suggestions are suggestions only; they are never exported unless accepted. Measured on synthetic audio only. |
| DC-21 | **Mitigated** | Renamed “Relative energy 1–10” in the app; the text says it is a rank within the library, not a perceived-energy rating. |
| DC-22 | **Documented** | Genre/style suggestions are never applied without review; per-genre results and weak genres are published. |
| DC-23 | **Documented** | Mood, voice and instrument labels are shown as model suggestions, not validated on DJ libraries. |
| DC-24 | **Documented** | Text search: Precision@10 19.6 % on genre queries; the app shows the figure and calls it a search aid. A DJ-judged query set is open. |
| DC-25 | **Documented** | Similar tracks: 35.6 % same-genre share in the top 10 (proxy measure, stated in the app). DJ-judged evaluation open. |
| DC-26 | **Documented** | Label suggestions show accuracy measured on the user's own labels; never trained on unconfirmed suggestions. |
| DC-27 | **Open** | Set Builder heuristics are not yet compared with DJs' choices. |
| DC-28 | **Documented** | Transcode detection is shown as “suspected” with the cut-off frequency; nothing is moved or deleted because of it. |
| DC-29 | **Mitigated** | Analysis results carry the analyser version; model outputs carry model and version (e.g. style vectors `1` vs `maest-1`, CLAP v2), so switching models re-flags tracks instead of mixing results. |

## Phase 3 — scalability and hardening

| ID | Status | Notes |
|---|---|---|
| DC-30 | **Documented** | The 100k benchmark uses synthetic rows (said in BENCHMARKS.md and on the site). A real 100k-file import is open (needs such a library). |
| DC-31 | **Open** | Analysis decodes whole tracks (≈ 56 MB per 5-minute track per worker); streaming decode is open. |
| DC-32 | **Open** | GPU paths (CUDA, DirectML, Core ML) are not exercised in CI (no GPU runners); CPU fallback is. |
| DC-33 | **Open** | No explicit scheduler giving the player priority over analysis yet; the “Light” profile limits analysis to one worker. |
| DC-34 | **Documented** | The Intel macOS build has no audio models; stated on the site, in the guide and in release notes. |
| DC-35 | **Documented** | Linux needs glibc 2.38 or newer (Ubuntu 24.04+); stated on the site, guide and release notes. |
| DC-40 | **Mitigated** | Checksums (`SHA256SUMS.txt`) are published with every release. Code signing and notarisation are open: they need an Apple Developer ID and a Windows code-signing certificate. |
| DC-41 | **Documented** | Every optional model shows its licence before download (non-commercial MTG models are marked); `LICENSING.md` lists origin, licence and hash. |
| DC-42 | **Fixed** | Model downloads: HTTPS only, size cap, written to a temporary file, BLAKE3 checked before the atomic rename, retries on 503/429; optional models (MAEST, CLAP) can be removed from the app. |
| DC-43 | **Mitigated** | Wording: the core works offline; network is used only when the user starts it (model download, online auto-tagger, whose sent fields are listed). |
| DC-44 | **Mitigated** | CI runs `cargo audit` and `npm audit` on every push. One advisory is excepted with a reason (lopdf, PDF parsing; Deep Cuts only writes PDFs). Fuzzing the XML/NML/CSV parsers is open. |
| DC-45 | **Fixed** | Watch-folder errors are shown in the app and kept in the audit log instead of stderr. |
| DC-46 | **Open** | Accessibility and onboarding with new users have not been tested. |
| DC-47 | **Mitigated** | `docs/RELEASE_CHECKLIST.md` defines the release gate; automated parts run in CI. |
| DC-48 | **Mitigated** | Stale statements corrected (migration test plan, release state); this page and the checklist are updated with every release. |

## Not done here, and what it would take

* Testing inside the real DJ applications (DC-13, DC-16) and on real audio devices (DC-38):
  machines with Rekordbox, Traktor, Serato and VirtualDJ, and a person to run the checklist.
* Code signing (DC-40): an Apple Developer ID (yearly fee) and a Windows code-signing certificate.
* Human-judged evaluations (DC-20, DC-24, DC-25, DC-27, DC-46): DJs rating results blind.
* GPU CI (DC-32): runners with NVIDIA and DirectML GPUs.
