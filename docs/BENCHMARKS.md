# Benchmarks

All numbers below were **measured** on 2026-10-20 in the development container: Linux x86-64,
4 vCPU, release build (`cargo build --release`). They are not projections. Desktop hardware will
differ; re-run with the commands given.

## Large library (synthetic rows, real code paths)

`cradex bench 100000 --dir <dir>` inserts 100 000 tracks through the same provenance + FTS code as
an import, then times queries returning the first page of 200 rows.

| Operation (100 000 tracks) | Time |
|---|---|
| Insert with field provenance + FTS | 43.8 s (one-off; real imports are bound by tag reading) |
| Unfiltered first page (default sort / by BPM) | 40 ms / 39 ms |
| Full-text term matching every track | 104 ms |
| `artist:"Artist 42"` | 0.8 ms |
| `bpm:123-125 key:8A` (823 hits) | 22 ms |
| `genre:tech -decision:keep` (33 899 hits) | 121 ms |
| `key:8A+ bpm:>120 (genre:techno OR genre:minimal)` | 69 ms |
| Dashboard statistics | 287 ms |

Before the two-phase paging change the unfiltered first page took 465–736 ms. That measurement drove
the change.

## Audio analysis (real encoded files)

| Input | Workers | Time |
|---|---|---|
| 1 × MP3 320 kbps, 5 min 20 s | 1 | 0.91 s analysis (1.3 s wall incl. process start) |
| 36 × MP3/FLAC/AIFF/M4A, 40–60 s each (30 min audio) | 3 | 2.0 s |

Peak memory per worker is dominated by the mono decode buffer (≈ 4 bytes × 44 100 × seconds, i.e.
~56 MB for a 5-minute track). Streaming analysis is listed as a later optimisation.

## Import

36 tagged files (MP3/FLAC/AIFF/M4A) imported in < 0.1 s including hashing. A 100k-file real-disk
import has **not** been measured yet (pending, see `TEST_PLAN.md`).
