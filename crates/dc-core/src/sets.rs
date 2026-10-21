//! Set preparation (no mixing, no audio rendering): chapters, pinned
//! tracks, transition compatibility, Magic Sort and next-track suggestions.

use crate::error::{Error, Result};
use crate::features::{self, Stats, Vector};
use crate::library::{self, TrackRow};
use crate::similarity::{self, Transition};
use crate::util::new_id;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const ROLES: &[&str] = &[
    "opening",
    "early-warmup",
    "building",
    "late-warmup",
    "peak",
    "closing",
    "custom",
];

/// Target energy (library percentile, 0..1) for each chapter role.
pub fn role_target(role: &str) -> Option<f32> {
    Some(match role {
        "opening" => 0.15,
        "early-warmup" => 0.3,
        "building" => 0.5,
        "late-warmup" => 0.62,
        "peak" => 0.88,
        "closing" => 0.4,
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub position: i64,
    pub role: String,
    pub name: Option<String>,
}

pub fn chapters(conn: &Connection, playlist: &str) -> Result<Vec<Chapter>> {
    let mut stmt = conn.prepare("SELECT id, position, role, name FROM set_chapters WHERE playlist_id = ?1 ORDER BY position")?;
    let rows = stmt.query_map([playlist], |r| {
        Ok(Chapter {
            id: r.get(0)?,
            position: r.get(1)?,
            role: r.get(2)?,
            name: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn set_chapter(
    conn: &Connection,
    playlist: &str,
    position: i64,
    role: &str,
    name: Option<&str>,
) -> Result<String> {
    if !ROLES.contains(&role) {
        return Err(Error::Invalid(format!("unknown chapter role '{role}'")));
    }
    if position < 0 {
        return Err(Error::Invalid("chapter position must be ≥ 0".into()));
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO set_chapters (id, playlist_id, position, role, name) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(playlist_id, position) DO UPDATE SET role = excluded.role, name = excluded.name",
        params![id, playlist, position, role, name],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM set_chapters WHERE playlist_id = ?1 AND position = ?2",
        params![playlist, position],
        |r| r.get(0),
    )?)
}

pub fn remove_chapter(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM set_chapters WHERE id = ?1", [id])?;
    Ok(())
}

pub fn locks(conn: &Connection, playlist: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT track_id FROM set_locks WHERE playlist_id = ?1")?;
    let rows = stmt.query_map([playlist], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn set_lock(conn: &Connection, playlist: &str, track: &str, locked: bool) -> Result<()> {
    if locked {
        conn.execute(
            "INSERT OR IGNORE INTO set_locks (playlist_id, track_id) VALUES (?1, ?2)",
            params![playlist, track],
        )?;
    } else {
        conn.execute(
            "DELETE FROM set_locks WHERE playlist_id = ?1 AND track_id = ?2",
            params![playlist, track],
        )?;
    }
    Ok(())
}

pub fn set_notes(conn: &Connection, playlist: &str, notes: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE playlists SET notes = ?1 WHERE id = ?2",
        params![notes, playlist],
    )?;
    Ok(())
}

/// Shared context: feature vectors, library stats, keys and energy percentiles.
pub struct Context {
    pub vecs: HashMap<String, Vector>,
    pub stats: Stats,
    pub keys: HashMap<String, String>,
    pub energy_pct: HashMap<String, f32>,
}

impl Context {
    pub fn load(conn: &Connection) -> Result<Context> {
        let vecs = features::all(conn)?;
        let stats = features::stats(vecs.values());
        let mut energies: Vec<(String, f32)> = vecs
            .iter()
            .filter_map(|(id, v)| features::energy_proxy(v, &stats).map(|e| (id.clone(), e)))
            .collect();
        energies.sort_by(|a, b| a.1.total_cmp(&b.1));
        let n = energies.len().max(2) as f32 - 1.0;
        let energy_pct = energies
            .into_iter()
            .enumerate()
            .map(|(i, (id, _))| (id, i as f32 / n))
            .collect();
        let mut stmt =
            conn.prepare("SELECT id, key_camelot FROM tracks WHERE key_camelot IS NOT NULL")?;
        let keys = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(Context {
            vecs,
            stats,
            keys,
            energy_pct,
        })
    }

    pub fn transition(&self, a: &str, b: &str) -> Transition {
        similarity::transition(
            self.vecs.get(a),
            self.vecs.get(b),
            self.keys.get(a).map(String::as_str),
            self.keys.get(b).map(String::as_str),
            &self.stats,
        )
    }

    fn timbre_sim(&self, a: &str, b: &str) -> f32 {
        match (self.vecs.get(a), self.vecs.get(b)) {
            (Some(x), Some(y)) => {
                let w = similarity::Weights {
                    tempo: 0.0,
                    harmony: 0.0,
                    energy: 0.0,
                    timbre: 0.6,
                    rhythm: 0.4,
                    dynamics: 0.0,
                    sound: 0.0,
                };
                similarity::score(x, y, &self.stats, None, None, &w).0
            }
            _ => 0.5,
        }
    }

    /// Cost of playing `b` after `a` (0 = ideal).
    fn step_cost(&self, a: &str, b: &str) -> f32 {
        (1.0 - self.transition(a, b).score) + 0.3 * (1.0 - self.timbre_sim(a, b))
    }

    fn target_cost(&self, t: &str, target: Option<f32>) -> f32 {
        match (target, self.energy_pct.get(t)) {
            // Strong enough to shape the set; transitions still break ties.
            (Some(g), Some(e)) => 2.5 * (e - g).abs(),
            _ => 0.0,
        }
    }
}

/// Energy target per position (chapters win over the mode).
pub fn targets(n: usize, chapters: &[Chapter], mode: &str) -> Vec<Option<f32>> {
    if n == 0 {
        return vec![];
    }
    if !chapters.is_empty() {
        return (0..n)
            .map(|i| {
                chapters
                    .iter()
                    .rfind(|c| c.position as usize <= i)
                    .and_then(|c| role_target(&c.role))
            })
            .collect();
    }
    let f = |i: usize| i as f32 / (n.max(2) - 1) as f32;
    match mode {
        "build" => (0..n).map(|i| Some(0.2 + 0.7 * f(i))).collect(),
        "arc" => (0..n)
            .map(|i| {
                let x = f(i);
                Some(if x < 0.7 {
                    0.2 + 0.7 * x / 0.7
                } else {
                    0.9 - 0.4 * (x - 0.7) / 0.3
                })
            })
            .collect(),
        _ => vec![None; n],
    }
}

fn total_cost(ctx: &Context, order: &[String], targets: &[Option<f32>]) -> f32 {
    let mut c = 0.0;
    for (i, t) in order.iter().enumerate() {
        if i > 0 {
            c += ctx.step_cost(&order[i - 1], t);
        }
        c += ctx.target_cost(t, targets.get(i).copied().flatten());
    }
    c
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SortResult {
    pub order: Vec<String>,
    pub cost_before: f32,
    pub cost_after: f32,
}

/// Propose a new order. Pinned tracks keep their positions. Nothing is
/// written; apply with [`apply_order`] (undoable).
pub fn magic_sort(conn: &Connection, playlist: &str, mode: &str) -> Result<SortResult> {
    let order = library::playlist_track_ids(conn, playlist)?;
    let ctx = Context::load(conn)?;
    let pinned = locks(conn, playlist)?;
    let tg = targets(order.len(), &chapters(conn, playlist)?, mode);
    Ok(sort_with(&ctx, &order, &pinned, &tg))
}

pub fn sort_with(
    ctx: &Context,
    order: &[String],
    pinned: &HashSet<String>,
    tg: &[Option<f32>],
) -> SortResult {
    let n = order.len();
    let before = total_cost(ctx, order, tg);
    let fixed: Vec<Option<String>> = order
        .iter()
        .map(|t| pinned.contains(t).then(|| t.clone()))
        .collect();
    let mut free: Vec<String> = order
        .iter()
        .filter(|t| !pinned.contains(*t))
        .cloned()
        .collect();
    // Greedy fill, looking one step ahead at a pinned successor.
    let mut out: Vec<String> = Vec::with_capacity(n);
    for p in 0..n {
        if let Some(t) = &fixed[p] {
            out.push(t.clone());
            continue;
        }
        let prev = out.last();
        let next_fixed = fixed.get(p + 1).cloned().flatten();
        let (best, _) = free
            .iter()
            .enumerate()
            .map(|(k, t)| {
                let mut c = ctx.target_cost(t, tg[p]);
                if let Some(pv) = prev {
                    c += ctx.step_cost(pv, t);
                } else if tg[p].is_none() {
                    // no target and no predecessor: start from the calmest track
                    c += ctx.energy_pct.get(t).copied().unwrap_or(0.5);
                }
                if let Some(nf) = &next_fixed {
                    c += ctx.step_cost(t, nf);
                }
                (k, c)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        out.push(free.remove(best));
    }
    // Local improvement (bounded): swap two free tracks, or move one free
    // track to another free slot shifting the free tracks in between.
    let movable: Vec<usize> = (0..n).filter(|&i| fixed[i].is_none()).collect();
    let mut cost = total_cost(ctx, &out, tg);
    let started = std::time::Instant::now();
    'outer: for _ in 0..50 {
        let mut improved = false;
        for ai in 0..movable.len() {
            for bi in 0..movable.len() {
                if ai == bi {
                    continue;
                }
                let (i, j) = (movable[ai], movable[bi]);
                // swap
                if ai < bi {
                    out.swap(i, j);
                    let c = total_cost(ctx, &out, tg);
                    if c + 1e-6 < cost {
                        cost = c;
                        improved = true;
                        continue;
                    }
                    out.swap(i, j);
                }
                // relocate free slot ai -> bi (rotate among free slots only)
                let mut trial = out.clone();
                let slots: Vec<usize> = if ai < bi {
                    movable[ai..=bi].to_vec()
                } else {
                    movable[bi..=ai].to_vec()
                };
                let mut vals: Vec<String> = slots.iter().map(|&p| trial[p].clone()).collect();
                if ai < bi {
                    vals.rotate_left(1);
                } else {
                    vals.rotate_right(1);
                }
                for (p, v) in slots.iter().zip(vals) {
                    trial[*p] = v;
                }
                let c = total_cost(ctx, &trial, tg);
                if c + 1e-6 < cost {
                    cost = c;
                    out = trial;
                    improved = true;
                }
                if started.elapsed().as_millis() > 600 {
                    break 'outer;
                }
            }
        }
        if !improved {
            break;
        }
    }
    // Never return something worse than the original.
    if cost > before {
        return SortResult {
            order: order.to_vec(),
            cost_before: before,
            cost_after: before,
        };
    }
    SortResult {
        order: out,
        cost_before: before,
        cost_after: cost,
    }
}

/// Apply a new order (undoable).
pub fn apply_order(conn: &Connection, playlist: &str, order: &[String]) -> Result<()> {
    let mut a = library::playlist_track_ids(conn, playlist)?;
    let mut b = order.to_vec();
    a.sort();
    b.sort();
    if a != b {
        return Err(Error::Invalid(
            "the new order must contain exactly the same tracks".into(),
        ));
    }
    library::reorder_playlist(conn, playlist, order, "set-sort")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTrack {
    pub track: TrackRow,
    pub energy: Option<f32>,
    pub locked: bool,
    pub chapter: Option<String>,
    pub target: Option<f32>,
    /// Transition from the previous track.
    pub transition: Option<Transition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetReport {
    pub id: String,
    pub name: String,
    pub notes: Option<String>,
    pub tracks: Vec<SetTrack>,
    pub chapters: Vec<Chapter>,
    pub duration_ms: i64,
    pub unanalyzed: usize,
    pub weak_transitions: usize,
}

pub fn report(conn: &Connection, playlist: &str, mode: &str) -> Result<SetReport> {
    let (name, notes): (String, Option<String>) = conn
        .query_row(
            "SELECT name, notes FROM playlists WHERE id = ?1",
            [playlist],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(format!("playlist {playlist}")))?;
    let ids = library::playlist_track_ids(conn, playlist)?;
    let rows = crate::query::rows_for_ids(conn, &ids)?;
    let ctx = Context::load(conn)?;
    let pinned = locks(conn, playlist)?;
    let chs = chapters(conn, playlist)?;
    let tg = targets(ids.len(), &chs, mode);
    let mut tracks = Vec::with_capacity(rows.len());
    for (i, t) in rows.into_iter().enumerate() {
        let chapter = chs
            .iter()
            .rfind(|c| c.position as usize <= i)
            .map(|c| c.role.clone());
        tracks.push(SetTrack {
            energy: ctx.energy_pct.get(&t.id).copied(),
            locked: pinned.contains(&t.id),
            chapter,
            target: tg.get(i).copied().flatten(),
            transition: (i > 0).then(|| ctx.transition(&ids[i - 1], &t.id)),
            track: t,
        });
    }
    Ok(SetReport {
        id: playlist.into(),
        name,
        notes,
        duration_ms: tracks.iter().filter_map(|t| t.track.duration_ms).sum(),
        unanalyzed: tracks
            .iter()
            .filter(|t| !ctx.vecs.contains_key(&t.track.id))
            .count(),
        weak_transitions: tracks
            .iter()
            .filter(|t| t.transition.as_ref().is_some_and(|x| x.score < 0.5))
            .count(),
        tracks,
        chapters: chs,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub track: TrackRow,
    pub transition: Transition,
    pub energy: Option<f32>,
    /// Combined fit (0..1): transition + closeness to the energy target.
    pub fit: f32,
}

/// Best next tracks after position `after` (or after the last track),
/// drawn from `scope` (query), excluding tracks in the set and global passes.
pub fn suggest_next(
    conn: &Connection,
    playlist: &str,
    after: Option<usize>,
    scope: &str,
    mode: &str,
    limit: usize,
) -> Result<Vec<Candidate>> {
    let ids = library::playlist_track_ids(conn, playlist)?;
    let Some(last) = after.and_then(|i| ids.get(i)).or(ids.last()).cloned() else {
        return Ok(vec![]);
    };
    let pos = after.map(|i| i + 1).unwrap_or(ids.len());
    let ctx = Context::load(conn)?;
    let chs = chapters(conn, playlist)?;
    let target = targets(ids.len() + 1, &chs, mode)
        .get(pos)
        .copied()
        .flatten();
    let in_set: HashSet<&String> = ids.iter().collect();
    let q = if scope.trim().is_empty() {
        "-decision:pass".to_string()
    } else {
        format!("({scope}) -decision:pass")
    };
    let pool = crate::query::query_track_ids(
        conn,
        &crate::query::TrackQuery {
            query: q,
            ..Default::default()
        },
    )?;
    let mut scored: Vec<(f32, String, Transition)> = pool
        .into_iter()
        .filter(|id| !in_set.contains(id) && ctx.vecs.contains_key(id))
        .map(|id| {
            let tr = ctx.transition(&last, &id);
            let fit = 0.75 * tr.score + 0.25 * (1.0 - ctx.target_cost(&id, target) / 2.5)
                - 0.1 * (1.0 - ctx.timbre_sim(&last, &id));
            (fit, id, tr)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(limit);
    let rows: HashMap<String, TrackRow> = crate::query::rows_for_ids(
        conn,
        &scored.iter().map(|s| s.1.clone()).collect::<Vec<_>>(),
    )?
    .into_iter()
    .map(|r| (r.id.clone(), r))
    .collect();
    Ok(scored
        .into_iter()
        .filter_map(|(fit, id, transition)| {
            rows.get(&id).map(|t| Candidate {
                energy: ctx.energy_pct.get(&id).copied(),
                track: t.clone(),
                transition,
                fit: fit.clamp(0.0, 1.0),
            })
        })
        .collect())
}

/// Build a set of `len` tracks starting from `seed` by walking the
/// transition graph (beam search over the same fit used by `suggest_next`:
/// transition score, energy target for `mode`, timbre continuity).
/// Candidates: analysed tracks matching `scope`, never passed ones.
pub fn build_path(
    conn: &Connection,
    seed: &str,
    len: usize,
    scope: &str,
    mode: &str,
) -> Result<Vec<String>> {
    let len = len.clamp(2, 200);
    let ctx = Context::load(conn)?;
    if !ctx.vecs.contains_key(seed) {
        return Err(crate::Error::Invalid(
            "the seed track is not analysed yet".into(),
        ));
    }
    let q = if scope.trim().is_empty() {
        "-decision:pass".to_string()
    } else {
        format!("({scope}) -decision:pass")
    };
    let pool: Vec<String> = crate::query::query_track_ids(
        conn,
        &crate::query::TrackQuery {
            query: q,
            ..Default::default()
        },
    )?
    .into_iter()
    .filter(|id| id != seed && ctx.vecs.contains_key(id))
    .collect();
    let target = targets(len, &[], mode);
    const BEAM: usize = 4;
    const BRANCH: usize = 6;
    // (total fit, path)
    let mut beam: Vec<(f32, Vec<String>)> = vec![(0.0, vec![seed.to_string()])];
    for &want in &target[1..] {
        let mut next: Vec<(f32, Vec<String>)> = Vec::new();
        for (score, path) in &beam {
            let last = path.last().unwrap();
            let used: HashSet<&String> = path.iter().collect();
            let mut cands: Vec<(f32, &String)> = pool
                .iter()
                .filter(|id| !used.contains(id))
                .map(|id| {
                    let tr = ctx.transition(last, id);
                    let fit = 0.75 * tr.score + 0.25 * (1.0 - ctx.target_cost(id, want) / 2.5)
                        - 0.1 * (1.0 - ctx.timbre_sim(last, id));
                    (fit, id)
                })
                .collect();
            cands.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (fit, id) in cands.into_iter().take(BRANCH) {
                let mut p = path.clone();
                p.push(id.clone());
                next.push((score + fit, p));
            }
        }
        if next.is_empty() {
            break; // pool exhausted
        }
        next.sort_by(|a, b| b.0.total_cmp(&a.0));
        next.truncate(BEAM);
        beam = next;
    }
    Ok(beam.into_iter().next().map(|b| b.1).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::N;

    fn ctx(spec: &[(&str, f32, &str, f32)]) -> Context {
        let mut vecs = HashMap::new();
        let mut keys = HashMap::new();
        for (id, bpm, key, lufs) in spec {
            let mut v = [0f32; N];
            v[0] = *bpm;
            v[4] = *lufs;
            v[10] = 4.0 + *lufs / 4.0;
            v[11] = 0.2;
            v[13] = *lufs - 4.0;
            v[7] = 10.5;
            vecs.insert(id.to_string(), v);
            keys.insert(id.to_string(), key.to_string());
        }
        let stats = features::stats(vecs.values());
        let mut e: Vec<(String, f32)> = vecs
            .iter()
            .map(|(id, v)| (id.clone(), features::energy_proxy(v, &stats).unwrap()))
            .collect();
        e.sort_by(|a, b| a.1.total_cmp(&b.1));
        let n = e.len() as f32 - 1.0;
        let energy_pct = e
            .into_iter()
            .enumerate()
            .map(|(i, (id, _))| (id, i as f32 / n))
            .collect();
        Context {
            vecs,
            stats,
            keys,
            energy_pct,
        }
    }

    #[test]
    fn targets_follow_chapters_then_mode() {
        let ch = vec![
            Chapter {
                id: "a".into(),
                position: 0,
                role: "opening".into(),
                name: None,
            },
            Chapter {
                id: "b".into(),
                position: 2,
                role: "peak".into(),
                name: None,
            },
        ];
        assert_eq!(
            targets(4, &ch, "smooth"),
            vec![Some(0.15), Some(0.15), Some(0.88), Some(0.88)]
        );
        let b = targets(3, &[], "build");
        assert!(b[0].unwrap() < b[2].unwrap());
        assert_eq!(targets(2, &[], "smooth"), vec![None, None]);
    }

    #[test]
    fn magic_sort_builds_energy_and_keeps_pins() {
        let c = ctx(&[
            ("loud", 128.0, "9A", -6.0),
            ("quiet", 122.0, "8A", -14.0),
            ("mid", 124.0, "8A", -10.0),
            ("mid2", 125.0, "9A", -9.0),
            ("pin", 126.0, "9A", -8.0),
        ]);
        let order: Vec<String> = ["loud", "pin", "quiet", "mid2", "mid"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let pinned: HashSet<String> = ["pin".to_string()].into();
        let tg = targets(order.len(), &[], "build");
        let r = sort_with(&c, &order, &pinned, &tg);
        assert_eq!(r.order[1], "pin", "pinned track keeps its position");
        assert_eq!(r.order[0], "quiet", "build starts calm: {:?}", r.order);
        assert_eq!(r.order[4], "loud", "and ends loud: {:?}", r.order);
        assert!(r.cost_after <= r.cost_before);
        let mut a = r.order.clone();
        let mut b = order.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b, "same tracks");
    }

    #[test]
    fn build_mode_rises_in_energy() {
        let spec: Vec<(String, f32, &str, f32)> = (0..12)
            .map(|i| {
                (
                    format!("t{i}"),
                    124.0,
                    if i % 2 == 0 { "8A" } else { "9A" },
                    -16.0 + i as f32,
                )
            })
            .collect();
        let refs: Vec<(&str, f32, &str, f32)> = spec
            .iter()
            .map(|(a, b, c, d)| (a.as_str(), *b, *c, *d))
            .collect();
        let c = ctx(&refs);
        // start from a scrambled order
        let order: Vec<String> = [5, 11, 0, 7, 2, 9, 4, 1, 10, 3, 8, 6]
            .iter()
            .map(|i| format!("t{i}"))
            .collect();
        let tg = targets(order.len(), &[], "build");
        let r = sort_with(&c, &order, &HashSet::new(), &tg);
        let e: Vec<f32> = r.order.iter().map(|id| c.energy_pct[id]).collect();
        // Spearman rank correlation between position and energy (energy
        // percentiles are already ranks). Small dips are allowed: harmonic
        // smoothness is traded against the target.
        let n = e.len() as f32;
        let ranks: Vec<f32> = e.iter().map(|x| x * (n - 1.0)).collect();
        let d2: f32 = ranks
            .iter()
            .enumerate()
            .map(|(i, r)| (i as f32 - r).powi(2))
            .sum();
        let rho = 1.0 - 6.0 * d2 / (n * (n * n - 1.0));
        assert!(
            rho > 0.9,
            "energy should rise along the set (rho {rho}): {e:?}"
        );
    }
}
