import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, DEFAULT_WEIGHTS, type DiscoveryMap, type Match, type SimilarityWeights, type TrackRow } from "../api";
import { KeyChip } from "../components/TrackTable";
import { camelotHue, formatBpm, trackLabel } from "../format";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

const GROUPS: (keyof SimilarityWeights)[] = ["tempo", "harmony", "energy", "timbre", "rhythm", "dynamics", "sound"];

function loadWeights(): SimilarityWeights {
  try {
    const w = JSON.parse(localStorage.getItem("dc.weights") ?? "null");
    if (w && GROUPS.every((g) => typeof w[g] === "number")) return w;
  } catch {
    /* default */
  }
  return DEFAULT_WEIGHTS;
}

/** Human-readable reasons for a match (measured facts only). */
export function matchNotes(m: Match, t: (k: MsgKey, v?: Record<string, string | number>) => string): string[] {
  const out: string[] = [];
  const f = m.facts;
  const [ba, bb] = f.bpm;
  if (ba != null && bb != null) out.push(t("disc.fact.bpm", { a: formatBpm(Math.round(ba * 10) / 10), b: formatBpm(Math.round(bb * 10) / 10) }));
  if (f.harmonic !== "unknown") out.push(t(`disc.harm.${f.harmonic}` as MsgKey, { a: f.camelot[0] ?? "?", b: f.camelot[1] ?? "?" }));
  const [la, lb] = f.lufs;
  if (la != null && lb != null && Math.abs(lb - la) >= 1.5) out.push(t(lb > la ? "disc.fact.louder" : "disc.fact.quieter", { d: Math.abs(lb - la).toFixed(1) }));
  const [ca, cb] = f.brightnessHz;
  if (ca != null && cb != null && Math.abs(Math.log2(cb / ca)) > 0.3) out.push(t(cb > ca ? "disc.fact.brighter" : "disc.fact.darker"));
  const [oa, ob] = f.onsets;
  if (oa != null && ob != null && Math.abs(ob - oa) > 1) out.push(t(ob > oa ? "disc.fact.busier" : "disc.fact.sparser"));
  const [da, db] = f.breakDepthDb;
  if (da != null && db != null && Math.abs(db - da) > 4) out.push(t(db > da ? "disc.fact.deeperBreaks" : "disc.fact.flatter"));
  return out;
}

function Similar() {
  const { t, discoverSeed, nowPlaying, play, dataVersion, prompt, bump } = useApp();
  const act = useAction();
  const [seed, setSeed] = useState<TrackRow | null>(discoverSeed ?? nowPlaying);
  const [weights, setWeights] = useState<SimilarityWeights>(loadWeights);
  const [scope, setScope] = useState("-decision:pass");
  const [results, setResults] = useState<Match[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (discoverSeed) setSeed(discoverSeed);
  }, [discoverSeed]);

  useEffect(() => {
    localStorage.setItem("dc.weights", JSON.stringify(weights));
  }, [weights]);

  useEffect(() => {
    if (!seed) return;
    const h = setTimeout(() => {
      api.similarTracks(seed.id, weights, scope, 60).then(
        (r) => {
          setResults(r);
          setError(null);
        },
        (e) => setError(String(e)),
      );
    }, 250);
    return () => clearTimeout(h);
  }, [seed, weights, scope, dataVersion]);

  const total = GROUPS.reduce((s, g) => s + weights[g], 0) || 1;

  return (
    <div className="discover-similar">
      <section className="panel">
        <div className="row space wrap">
          <div>
            <div className="section-title">{t("disc.seed")}</div>
            {seed ? (
              <div className="seed">
                <button className="icon-btn" onClick={() => play(seed, seed.previewMs ?? 0)} aria-label={t("player.play")}>
                  ▶
                </button>
                <strong>{trackLabel(seed)}</strong>
                <span className="num muted">{formatBpm(seed.bpm)}</span>
                <KeyChip t={seed} />
              </div>
            ) : (
              <p className="muted">{t("disc.noSeed")}</p>
            )}
          </div>
          {nowPlaying && nowPlaying.id !== seed?.id && (
            <button className="btn ghost" onClick={() => setSeed(nowPlaying)}>
              {t("disc.usePlaying")}
            </button>
          )}
        </div>
        <div className="weights">
          {GROUPS.map((g) => (
            <label key={g} className="weight" title={t(`disc.groupHint.${g}` as MsgKey)}>
              <span>{t(`disc.group.${g}` as MsgKey)}</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={weights[g]}
                onChange={(e) => setWeights((w) => ({ ...w, [g]: Number(e.target.value) }))}
              />
              <span className="num muted">{Math.round((weights[g] / total) * 100)}%</span>
            </label>
          ))}
          <button className="link-btn" onClick={() => setWeights(DEFAULT_WEIGHTS)}>
            {t("disc.resetWeights")}
          </button>
        </div>
        <div className="row mt">
          <input className="mono grow" value={scope} onChange={(e) => setScope(e.target.value)} placeholder={t("disc.scope")} spellCheck={false} />
        </div>
        <p className="hint">{t("disc.honesty")}</p>
      </section>
      {error && <div className="query-error">{error}</div>}
      {results && (
        <section className="panel">
          <div className="row space">
            <h2>{t("disc.results", { n: results.length })}</h2>
            {results.length > 0 && (
              <button
                className="btn small ghost"
                onClick={async () => {
                  const name = await prompt(t("library.newPlaylistName"), seed ? `${t("disc.like")} ${seed.title ?? ""}` : "");
                  if (name) act(() => api.createPlaylist(name, "manual", null, [seed!.id, ...results.slice(0, 25).map((r) => r.track.id)]), t("library.playlistCreated", { name })).then(bump);
                }}
              >
                {t("disc.toPlaylist")}
              </button>
            )}
          </div>
          <ul className="match-list">
            {results.map((m) => (
              <li key={m.track.id} className="match">
                <div className="match-score" title={`${Math.round(m.score * 100)}%`}>
                  <span className="num">{Math.round(m.score * 100)}</span>
                  <div className="meter">
                    <div style={{ width: `${m.score * 100}%` }} />
                  </div>
                </div>
                <button className="icon-btn" onClick={() => play(m.track, m.track.previewMs ?? 0)} aria-label={t("player.play")}>
                  ▶
                </button>
                <div className="match-main">
                  <div className="match-title">
                    {trackLabel(m.track)} {m.track.decision && <span className={`dec-dot dec-${m.track.decision}`} />}
                  </div>
                  <div className="match-notes muted small">{matchNotes(m, t).join(" · ")}</div>
                </div>
                <div className="group-bars" aria-label="dimensions">
                  {m.groups.map((g) => (
                    <div key={g.group} className="gbar" title={`${t(`disc.group.${g.group}` as MsgKey)}: ${g.similarity == null ? "—" : Math.round(g.similarity * 100) + "%"}`}>
                      <div className="gbar-fill" style={{ height: `${(g.similarity ?? 0) * 100}%`, opacity: g.similarity == null ? 0.2 : 1 }} />
                    </div>
                  ))}
                </div>
                <button className="btn small ghost" onClick={() => setSeed(m.track)}>
                  {t("disc.useAsSeed")}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function MapView() {
  const { t, play, dataVersion, prompt, bump } = useApp();
  const act = useAction();
  const canvas = useRef<HTMLCanvasElement>(null);
  const [scope, setScope] = useState("");
  const [data, setData] = useState<DiscoveryMap | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState({ x: 0, y: 0, k: 1 });
  const [hover, setHover] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [colorBy, setColorBy] = useState<"key" | "decision" | "bpm">("key");
  const drag = useRef<{ x: number; y: number; vx: number; vy: number; rect: boolean } | null>(null);
  const [rect, setRect] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);

  useEffect(() => {
    const h = setTimeout(() => {
      api.discoveryMap(scope).then(
        (d) => {
          setData(d);
          setError(null);
        },
        (e) => setError(String(e)),
      );
    }, 300);
    return () => clearTimeout(h);
  }, [scope, dataVersion]);

  const bounds = useMemo(() => {
    if (!data || data.points.length === 0) return { minX: -1, maxX: 1, minY: -1, maxY: 1 };
    const xs = data.points.map((p) => p.x);
    const ys = data.points.map((p) => p.y);
    return { minX: Math.min(...xs), maxX: Math.max(...xs), minY: Math.min(...ys), maxY: Math.max(...ys) };
  }, [data]);

  const toScreen = useCallback(
    (px: number, py: number, w: number, h: number) => {
      const pad = 30;
      const sx = pad + ((px - bounds.minX) / (bounds.maxX - bounds.minX || 1)) * (w - 2 * pad);
      const sy = h - pad - ((py - bounds.minY) / (bounds.maxY - bounds.minY || 1)) * (h - 2 * pad);
      return [(sx - w / 2) * view.k + w / 2 + view.x, (sy - h / 2) * view.k + h / 2 + view.y];
    },
    [bounds, view],
  );

  const color = useCallback(
    (p: DiscoveryMap["points"][number]) => {
      if (colorBy === "decision") return p.decision === "keep" ? "#3ddc84" : p.decision === "pass" ? "#ff5c6c" : p.decision === "maybe" ? "#ffc23d" : "#6b7280";
      if (colorBy === "bpm") {
        const b = p.bpm ?? 0;
        const x = Math.max(0, Math.min(1, (b - 100) / 50));
        return `hsl(${220 - x * 200} 80% 60%)`;
      }
      const hue = camelotHue(p.camelot);
      return hue == null ? "#6b7280" : `hsl(${hue} 75% 62%)`;
    },
    [colorBy],
  );

  useEffect(() => {
    const c = canvas.current;
    if (!c || !data) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = w * dpr;
    c.height = h * dpr;
    const g = c.getContext("2d")!;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, w, h);
    const r = Math.max(2, Math.min(6, 1400 / Math.sqrt(data.points.length + 1) / 10)) * Math.sqrt(view.k);
    data.points.forEach((p, i) => {
      const [x, y] = toScreen(p.x, p.y, w, h);
      if (x < -10 || y < -10 || x > w + 10 || y > h + 10) return;
      g.beginPath();
      g.arc(x, y, selected.has(p.id) ? r + 2 : r, 0, Math.PI * 2);
      g.fillStyle = color(p);
      g.globalAlpha = selected.size && !selected.has(p.id) ? 0.35 : 0.9;
      g.fill();
      if (selected.has(p.id) || hover === i) {
        g.globalAlpha = 1;
        g.lineWidth = 2;
        g.strokeStyle = "#ffffff";
        g.stroke();
      }
    });
    g.globalAlpha = 1;
    if (rect) {
      g.strokeStyle = "#c6ff3d";
      g.setLineDash([4, 4]);
      g.strokeRect(rect.x0, rect.y0, rect.x1 - rect.x0, rect.y1 - rect.y0);
      g.setLineDash([]);
    }
  }, [data, view, hover, selected, rect, toScreen, color]);

  const pick = (ev: { clientX: number; clientY: number }) => {
    const c = canvas.current;
    if (!c || !data) return null;
    const b = c.getBoundingClientRect();
    const mx = ev.clientX - b.left;
    const my = ev.clientY - b.top;
    let best: number | null = null;
    let bd = 100;
    data.points.forEach((p, i) => {
      const [x, y] = toScreen(p.x, p.y, c.clientWidth, c.clientHeight);
      const d = (x - mx) ** 2 + (y - my) ** 2;
      if (d < bd) {
        bd = d;
        best = i;
      }
    });
    return best;
  };

  const axisLabel = (i: number) => {
    const a = data?.axes[i];
    if (!a) return "";
    return `${Math.round(a.explained * 100)}% · ${a.top.map(([d, w]) => `${w >= 0 ? "+" : "−"}${t(`dim.${d}` as MsgKey)}`).join(", ")}`;
  };

  const hovered = hover != null ? data?.points[hover] : null;

  return (
    <div className="discover-map">
      <div className="row wrap map-toolbar">
        <input className="mono grow" value={scope} onChange={(e) => setScope(e.target.value)} placeholder={t("disc.mapScope")} spellCheck={false} />
        <label className="field-inline">
          {t("disc.colorBy")}
          <select value={colorBy} onChange={(e) => setColorBy(e.target.value as typeof colorBy)}>
            <option value="key">{t("col.key")}</option>
            <option value="bpm">BPM</option>
            <option value="decision">{t("col.decision")}</option>
          </select>
        </label>
        <button className="btn small ghost" onClick={() => setView({ x: 0, y: 0, k: 1 })}>
          {t("disc.resetView")}
        </button>
        <span className="muted small">
          {data ? t("disc.mapCount", { n: data.points.length, missing: data.missing }) : t("common.loading")}
        </span>
        {selected.size > 0 && (
          <>
            <span className="badge">{t("library.selected")}: {selected.size}</span>
            <button
              className="btn small"
              onClick={async () => {
                const name = await prompt(t("library.newPlaylistName"));
                if (name) act(() => api.createPlaylist(name, "manual", null, [...selected]), t("library.playlistCreated", { name })).then(bump);
              }}
            >
              {t("disc.toPlaylist")}
            </button>
            <button className="btn small ghost" onClick={() => setSelected(new Set())}>
              {t("action.clear")}
            </button>
          </>
        )}
      </div>
      {error && <div className="query-error">{error}</div>}
      <div className="map-wrap">
        <canvas
          ref={canvas}
          className="map-canvas"
          onWheel={(e) => {
            const f = e.deltaY < 0 ? 1.15 : 1 / 1.15;
            setView((v) => ({ ...v, k: Math.max(0.5, Math.min(40, v.k * f)) }));
          }}
          onMouseDown={(e) => {
            drag.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, rect: e.shiftKey };
            if (e.shiftKey) {
              const b = e.currentTarget.getBoundingClientRect();
              setRect({ x0: e.clientX - b.left, y0: e.clientY - b.top, x1: e.clientX - b.left, y1: e.clientY - b.top });
            }
          }}
          onMouseMove={(e) => {
            const d = drag.current;
            if (d && d.rect) {
              const b = e.currentTarget.getBoundingClientRect();
              setRect((r) => (r ? { ...r, x1: e.clientX - b.left, y1: e.clientY - b.top } : r));
            } else if (d) {
              setView((v) => ({ ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y }));
            } else setHover(pick(e));
          }}
          onMouseUp={(e) => {
            const d = drag.current;
            drag.current = null;
            const c = canvas.current;
            if (d?.rect && rect && c && data) {
              const [x0, x1] = [Math.min(rect.x0, rect.x1), Math.max(rect.x0, rect.x1)];
              const [y0, y1] = [Math.min(rect.y0, rect.y1), Math.max(rect.y0, rect.y1)];
              const ids = data.points
                .filter((p) => {
                  const [x, y] = toScreen(p.x, p.y, c.clientWidth, c.clientHeight);
                  return x >= x0 && x <= x1 && y >= y0 && y <= y1;
                })
                .map((p) => p.id);
              setSelected((s) => new Set([...s, ...ids]));
              setRect(null);
              return;
            }
            if (d && Math.abs(e.clientX - d.x) + Math.abs(e.clientY - d.y) < 4) {
              const i = pick(e);
              if (i != null && data) {
                const p = data.points[i];
                api.trackDetail(p.id).then((det) => play(det.track, det.track.previewMs ?? 0), () => {});
                setSelected((s) => {
                  const n = new Set(e.metaKey || e.ctrlKey ? s : []);
                  n.add(p.id);
                  return n;
                });
              }
            }
          }}
          onMouseLeave={() => {
            drag.current = null;
            setHover(null);
            setRect(null);
          }}
        />
        {hovered && (
          <div className="map-tip">
            <strong>{hovered.label}</strong>
            <span className="num muted">
              {formatBpm(hovered.bpm)} {hovered.camelot}
            </span>
          </div>
        )}
        <div className="map-axis map-axis-x">→ {axisLabel(0)}</div>
        <div className="map-axis map-axis-y">↑ {axisLabel(1)}</div>
      </div>
      <p className="hint">{t("disc.mapHint")}</p>
    </div>
  );
}

export function Discover() {
  const { t } = useApp();
  const [tab, setTab] = useState<"similar" | "map">("similar");
  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.discover")}</h1>
        <div className="seg">
          <button className={tab === "similar" ? "active-tab" : ""} onClick={() => setTab("similar")}>
            {t("disc.tabSimilar")}
          </button>
          <button className={tab === "map" ? "active-tab" : ""} onClick={() => setTab("map")}>
            {t("disc.tabMap")}
          </button>
        </div>
        <span className="badge">{t("disc.measuredBadge")}</span>
      </header>
      {tab === "similar" ? <Similar /> : <MapView />}
    </div>
  );
}
