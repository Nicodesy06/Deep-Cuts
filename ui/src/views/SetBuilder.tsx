// Set preparation: order, chapters, pinned tracks, transitions, suggestions.
// Preparation only — nothing here mixes or renders audio.
import { useCallback, useEffect, useState } from "react";
import { api, CHAPTER_ROLES, type Candidate, type Playlist, type SetReport, type Transition } from "../api";
import { exportTracks } from "../actions";
import { KeyChip } from "../components/TrackTable";
import { formatBpm, formatDuration, trackLabel } from "../format";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

export const MODES = ["smooth", "build", "arc"] as const;

function TransitionBadge({ tr }: { tr: Transition }) {
  const { t } = useApp();
  const cls = tr.score >= 0.75 ? "good" : tr.score >= 0.5 ? "ok" : "weak";
  const bpm = tr.bpmChangePct == null ? "" : `${tr.bpmChangePct >= 0 ? "+" : ""}${tr.bpmChangePct.toFixed(1)}%`;
  return (
    <span className={`trans trans-${cls}`} title={t("sets.transHint")}>
      {t(`sets.harm.${tr.harmonic}` as MsgKey)} {bpm}
    </span>
  );
}

/** Small multiples: energy (with target) and BPM along the set. */
function Progression({ rep }: { rep: SetReport }) {
  const { t } = useApp();
  const n = rep.tracks.length;
  if (n < 2) return null;
  const W = 600;
  const H = 70;
  const x = (i: number) => (i / (n - 1)) * W;
  const line = (vals: (number | null)[], lo: number, hi: number) =>
    vals
      .map((v, i) => (v == null ? null : `${x(i).toFixed(1)},${(H - ((v - lo) / (hi - lo || 1)) * (H - 8) - 4).toFixed(1)}`))
      .filter(Boolean)
      .join(" ");
  const bpms = rep.tracks.map((s) => s.track.bpm);
  const valid = bpms.filter((b): b is number => b != null);
  const [bl, bh] = valid.length ? [Math.min(...valid) - 1, Math.max(...valid) + 1] : [0, 1];
  return (
    <div className="progression">
      {[
        { title: t("sets.energy"), main: rep.tracks.map((s) => s.energy), target: rep.tracks.map((s) => s.target), lo: 0, hi: 1, fmt: "" },
        { title: "BPM", main: bpms, target: null, lo: bl, hi: bh, fmt: `${formatBpm(Math.round(bl + 1))}–${formatBpm(Math.round(bh - 1))}` },
      ].map((c) => (
        <figure key={c.title} className="prog-chart">
          <figcaption>
            <span>{c.title}</span>
            <span className="muted small">{c.fmt}</span>
          </figcaption>
          <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={c.title}>
            {rep.chapters.map((ch) => (
              <line key={ch.id} x1={x(ch.position)} x2={x(ch.position)} y1={0} y2={H} className="prog-chapter" />
            ))}
            {c.target && <polyline points={line(c.target, c.lo, c.hi)} className="prog-target" />}
            <polyline points={line(c.main, c.lo, c.hi)} className="prog-line" />
          </svg>
        </figure>
      ))}
    </div>
  );
}

export function SetBuilder() {
  const { t, setId, openSet, dataVersion, bump, play, prompt, confirm, toast } = useApp();
  const act = useAction();
  const [sets, setSets] = useState<Playlist[]>([]);
  const [rep, setRep] = useState<SetReport | null>(null);
  const [mode, setMode] = useState<string>(() => localStorage.getItem("dc.setMode") ?? "smooth");
  const [focus, setFocus] = useState<number | null>(null);
  const [scope, setScope] = useState("");
  const [cands, setCands] = useState<Candidate[] | null>(null);
  const [preview, setPreview] = useState<{ order: string[]; costBefore: number; costAfter: number } | null>(null);
  const [notes, setNotes] = useState("");

  useEffect(() => {
    api.listPlaylists().then((ps) => {
      const s = ps.filter((p) => p.kind === "set");
      setSets(s);
      if (!setId && s.length) openSet(s[0].id);
    }, () => {});
  }, [dataVersion]);

  const reload = useCallback(() => {
    if (!setId) return setRep(null);
    api.setReport(setId, mode).then(
      (r) => {
        setRep(r);
        setNotes(r.notes ?? "");
      },
      (e) => toast(String(e), "error"),
    );
  }, [setId, mode, toast]);

  useEffect(reload, [reload, dataVersion]);
  useEffect(() => localStorage.setItem("dc.setMode", mode), [mode]);

  useEffect(() => {
    if (!setId || !rep || rep.tracks.length === 0) return setCands(null);
    const h = setTimeout(() => {
      api.suggestNext(setId, focus, scope, mode, 15).then(setCands, () => setCands([]));
    }, 250);
    return () => clearTimeout(h);
  }, [setId, rep, focus, scope, mode]);

  const ids = rep?.tracks.map((s) => s.track.id) ?? [];
  const reorder = (next: string[]) => act(() => api.setPlaylistOrder(setId!, next)).then(bump);
  const move = (i: number, d: number) => {
    const j = i + d;
    if (j < 0 || j >= ids.length) return;
    const next = [...ids];
    [next[i], next[j]] = [next[j], next[i]];
    setFocus(j);
    reorder(next);
  };
  const insertAfter = (id: string) => {
    const at = focus == null ? ids.length : focus + 1;
    const next = [...ids.slice(0, at), id, ...ids.slice(at)];
    setFocus(at);
    reorder(next);
  };

  const newSet = async () => {
    const name = await prompt(t("sets.newName"));
    if (!name) return;
    const id = await act(() => api.createPlaylist(name, "set"));
    if (id) {
      openSet(id);
      bump();
    }
  };

  return (
    <div className="page set-builder">
      <header className="page-head">
        <h1>{t("nav.sets")}</h1>
        <select value={setId ?? ""} onChange={(e) => openSet(e.target.value)} aria-label={t("sets.choose")}>
          {sets.length === 0 && <option value="">{t("sets.none")}</option>}
          {sets.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name} ({s.trackCount})
            </option>
          ))}
        </select>
        <button className="btn small" onClick={newSet}>
          + {t("sets.new")}
        </button>
        <span className="muted small">{t("sets.subtitle")}</span>
      </header>

      {!rep ? (
        <section className="panel">
          <p className="muted">{t("sets.empty")}</p>
        </section>
      ) : (
        <div className="set-grid">
          <div className="set-main">
            <section className="panel">
              <div className="row space wrap">
                <div className="row wrap">
                  <strong>{rep.name}</strong>
                  <span className="chip num">{rep.tracks.length} {t("library.tracks")}</span>
                  <span className="chip num">{formatDuration(rep.durationMs)}</span>
                  {rep.weakTransitions > 0 && <span className="badge badge-warn">{t("sets.weak", { n: rep.weakTransitions })}</span>}
                  {rep.unanalyzed > 0 && <span className="badge">{t("sets.unanalyzed", { n: rep.unanalyzed })}</span>}
                </div>
                <div className="row wrap">
                  <label className="field-inline" title={t("sets.modeHint")}>
                    {t("sets.mode")}
                    <select value={mode} onChange={(e) => setMode(e.target.value)}>
                      {MODES.map((m) => (
                        <option key={m} value={m}>
                          {t(`sets.mode.${m}` as MsgKey)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <button className="btn" disabled={rep.tracks.length < 3} onClick={() => act(() => api.magicSort(rep.id, mode)).then((r) => r && setPreview(r))}>
                    {t("sets.magicSort")}
                  </button>
                  <button className="btn ghost small" onClick={() => exportTracks(t, toast, "rekordbox", { query: { query: "", playlistId: rep.id } }, rep.name)}>
                    {t("export.rekordbox")}
                  </button>
                  <button className="btn ghost small" onClick={() => exportTracks(t, toast, "m3u8", { query: { query: "", playlistId: rep.id } }, rep.name)}>
                    M3U8
                  </button>
                </div>
              </div>
              {preview && (
                <div className="sort-preview">
                  <span>
                    {t("sets.sortPreview", { before: preview.costBefore.toFixed(2), after: preview.costAfter.toFixed(2) })}
                  </span>
                  <div className="row">
                    <button className="btn small" onClick={() => act(() => api.applySetOrder(rep.id, preview.order), t("sets.sorted")).then(() => (setPreview(null), bump()))}>
                      {t("details.apply")}
                    </button>
                    <button className="btn small ghost" onClick={() => setPreview(null)}>
                      {t("action.cancel")}
                    </button>
                  </div>
                  <ol className="small muted preview-order">
                    {preview.order.map((id) => {
                      const s = rep.tracks.find((x) => x.track.id === id);
                      return <li key={id}>{s ? trackLabel(s.track) : id}</li>;
                    })}
                  </ol>
                </div>
              )}
              <Progression rep={rep} />
            </section>

            <section className="panel set-list">
              {rep.tracks.length === 0 && <p className="muted">{t("sets.emptySet")}</p>}
              <ol>
                {rep.tracks.map((s, i) => {
                  const ch = rep.chapters.find((c) => c.position === i);
                  return (
                    <li key={s.track.id + i}>
                      {ch && (
                        <div className="chapter-head">
                          <span className="chapter-role">{t(`sets.role.${ch.role}` as MsgKey)}</span>
                          {ch.name && <span className="muted">{ch.name}</span>}
                          <button className="link-btn small" onClick={() => act(() => api.removeChapter(ch.id)).then(bump)}>
                            ×
                          </button>
                        </div>
                      )}
                      {s.transition && (
                        <div className="trans-row">
                          <TransitionBadge tr={s.transition} />
                        </div>
                      )}
                      <div className={`set-row ${focus === i ? "focus" : ""}`} onClick={() => setFocus(i)}>
                        <span className="num muted">{i + 1}</span>
                        <button
                          className={`icon-btn small lock ${s.locked ? "on" : ""}`}
                          title={t("sets.lockHint")}
                          onClick={(e) => (e.stopPropagation(), act(() => api.setTrackLock(rep.id, s.track.id, !s.locked)).then(bump))}
                        >
                          {s.locked ? "🔒" : "🔓"}
                        </button>
                        <button className="icon-btn small" onClick={(e) => (e.stopPropagation(), play(s.track, s.track.previewMs ?? 0))} aria-label={t("player.play")}>
                          ▶
                        </button>
                        <span className="set-title">{trackLabel(s.track)}</span>
                        <span className="num">{formatBpm(s.track.bpm)}</span>
                        <KeyChip t={s.track} />
                        <span className="energy-bar" title={s.energy == null ? "—" : `${t("sets.energy")}: ${Math.round(s.energy * 100)}`}>
                          <span style={{ width: `${(s.energy ?? 0) * 100}%` }} />
                          {s.target != null && <i style={{ left: `${s.target * 100}%` }} />}
                        </span>
                        <span className="row-actions">
                          <button className="icon-btn small" onClick={(e) => (e.stopPropagation(), move(i, -1))} aria-label="up">
                            ↑
                          </button>
                          <button className="icon-btn small" onClick={(e) => (e.stopPropagation(), move(i, 1))} aria-label="down">
                            ↓
                          </button>
                          <select
                            className="chapter-select"
                            value=""
                            onClick={(e) => e.stopPropagation()}
                            onChange={(e) => e.target.value && act(() => api.setChapter(rep.id, i, e.target.value)).then(bump)}
                            aria-label={t("sets.startChapter")}
                          >
                            <option value="">{t("sets.startChapter")}</option>
                            {CHAPTER_ROLES.map((r) => (
                              <option key={r} value={r}>
                                {t(`sets.role.${r}` as MsgKey)}
                              </option>
                            ))}
                          </select>
                          <button
                            className="icon-btn small"
                            onClick={async (e) => {
                              e.stopPropagation();
                              if (await confirm(t("sets.removeQ", { name: trackLabel(s.track) }))) reorder(ids.filter((_, k) => k !== i));
                            }}
                            aria-label={t("action.delete")}
                          >
                            ×
                          </button>
                        </span>
                      </div>
                    </li>
                  );
                })}
              </ol>
            </section>
            <section className="panel">
              <label className="field col">
                <span className="field-label">{t("sets.notes")}</span>
                <textarea rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} onBlur={() => act(() => api.setPlaylistNotes(rep.id, notes || null))} />
              </label>
            </section>
          </div>

          <aside className="panel set-suggest">
            <h2>{focus == null ? t("sets.nextAfterLast") : t("sets.nextAfter", { n: focus + 1 })}</h2>
            <input className="mono" value={scope} onChange={(e) => setScope(e.target.value)} placeholder={t("sets.scope")} spellCheck={false} />
            <p className="hint">{t("sets.suggestHint")}</p>
            {cands == null ? null : cands.length === 0 ? (
              <p className="muted">{t("sets.noCandidates")}</p>
            ) : (
              <ul className="cand-list">
                {cands.map((c) => (
                  <li key={c.track.id}>
                    <div className="row">
                      <span className="num fit">{Math.round(c.fit * 100)}</span>
                      <button className="icon-btn small" onClick={() => play(c.track, c.track.previewMs ?? 0)} aria-label={t("player.play")}>
                        ▶
                      </button>
                      <span className="cand-title">{trackLabel(c.track)}</span>
                    </div>
                    <div className="row small">
                      <span className="num muted">{formatBpm(c.track.bpm)}</span>
                      <KeyChip t={c.track} />
                      <TransitionBadge tr={c.transition} />
                      <span className="spacer" />
                      <button className="btn small ghost" onClick={() => insertAfter(c.track.id)}>
                        + {t("sets.add")}
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </aside>
        </div>
      )}
    </div>
  );
}
