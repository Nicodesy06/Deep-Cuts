# Continuity with `doureallywanttokeepit?`

## Audit result (2026-10-20)

The repository was **not available**. It is not among the repositories this workspace can access
(checked via the GitHub integration), and `Deep-Cuts` started empty. Nothing was assumed about its
implementation. In particular, none of its AI modules were assumed to exist or to be validated.

## How Deep Cuts already accommodates its data

| Legacy concept | Deep Cuts equivalent |
|---|---|
| Rekordbox library import | Rekordbox XML import (ids kept in `external_ids`) |
| Keep / Pass / Maybe | `decisions` (identical values) |
| Decisions per Taste Profile / Set DNA | `decisions.context` (e.g. `profile:<name>`, `set:<name>`), never a global exclusion |
| Ratings, notes | `track_fields(field='rating', source=…)`, `tracks.notes` |
| Personal classifications / tags | `tags` + `track_tags(origin='user')` |
| HOT preview | measured hot region (`tracks.preview_ms`) + HOT button / `H` key |
| Track identity | track UUID + content hash; legacy ids would be stored as `external_ids(source='dourwtki')` |

## Migration plan (once the repository is shared)

1. Read-only audit of its storage format (SQLite/JSON/…) and of which features really work.
2. Write `deepcuts import-legacy <path>`: match legacy tracks to Deep Cuts tracks by path, then by
   Rekordbox TrackID, then by content hash. Import decisions, ratings, notes, tags, taste profiles and
   playlists as `import:dourwtki` / user values. Back up first and print a report of unmatched items.
3. Never write to the legacy data.
