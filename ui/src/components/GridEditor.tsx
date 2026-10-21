// Beatgrid editor: zoomed audio around the playhead with beat lines,
// downbeat at playhead, nudge, BPM ×2 ÷2, tap tempo, reset. All undoable;
// the user grid wins over imported and analysed grids (and is exported).
import { useEffect, useRef, useState } from "react";
import { api, type TrackRow } from "../api";
import { useAction, useApp } from "../store";

const WINDOW_MS = 4000;
const POINTS = 400;

type Grid = { source: string; bpm: number; firstBeatMs: number };

export function effectiveGrid(grids: Grid[]): Grid | null {
  const rank = (s: string) => (s === "user" ? 0 : s === "analysis" ? 2 : 1);
  return [...grids].sort((a, b) => rank(a.source) - rank(b.source))[0] ?? null;
}

/// Bpm from tap times (ms): median interval of the last taps.
export function tapBpm(taps: number[]): number | null {
  if (taps.length < 4) return null;
  const iv = taps
    .slice(-9)
    .slice(1)
    .map((t, i, a) => t - (i === 0 ? taps.slice(-9)[0] : a[i - 1]))
    .sort((a, b) => a - b);
  const med = iv[Math.floor(iv.length / 2)];
  return med > 0 ? Math.round((60000 / med) * 10) / 10 : null;
}

export function GridEditor({ track, grids, onChanged }: { track: TrackRow; grids: Grid[]; onChanged: () => void }) {
  const { t, playerPosRef, nowPlaying } = useApp();
  const act = useAction();
  const grid = effectiveGrid(grids);
  const [center, setCenter] = useState<number | null>(null);
  const [env, setEnv] = useState<[number, number][] | null>(null);
  const [bpmInput, setBpmInput] = useState("");
  const [taps, setTaps] = useState<number[]>([]);
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    setCenter(null);
    setTaps([]);
  }, [track.id]);
  useEffect(() => setBpmInput(grid ? grid.bpm.toFixed(2) : ""), [grid?.bpm]);

  const start = center != null ? Math.max(0, center - WINDOW_MS / 2) : null;
  useEffect(() => {
    if (start == null) return setEnv(null);
    let live = true;
    api.gridWindow(track.id, start, WINDOW_MS, POINTS).then((e) => live && setEnv(e), () => live && setEnv(null));
    return () => {
      live = false;
    };
  }, [track.id, start]);

  // Draw envelope + beat lines + playhead.
  useEffect(() => {
    const c = canvas.current;
    if (!c || !env || start == null) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = w * dpr;
    c.height = h * dpr;
    const g = c.getContext("2d")!;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, w, h);
    const css = getComputedStyle(c);
    const accent = css.getPropertyValue("--accent").trim() || "#c6ff3d";
    const muted = css.getPropertyValue("--muted").trim() || "#888";
    const maxFull = Math.max(1e-6, ...env.map((e) => e[0]));
    const maxLow = Math.max(1e-6, ...env.map((e) => e[1]));
    for (let i = 0; i < env.length; i++) {
      const x = (i / env.length) * w;
      const fh = (env[i][0] / maxFull) * (h / 2 - 2);
      const lh = (env[i][1] / maxLow) * (h / 2 - 2);
      g.fillStyle = muted;
      g.globalAlpha = 0.45;
      g.fillRect(x, h / 2 - fh, Math.max(1, w / env.length), fh * 2);
      g.globalAlpha = 0.9;
      g.fillStyle = "#58d6ff";
      g.fillRect(x, h / 2 - lh, Math.max(1, w / env.length), lh * 2);
    }
    g.globalAlpha = 1;
    if (grid) {
      const p = 60000 / grid.bpm;
      const k0 = Math.ceil((start - grid.firstBeatMs) / p);
      for (let k = k0; ; k++) {
        const ms = grid.firstBeatMs + k * p;
        if (ms > start + WINDOW_MS) break;
        const x = ((ms - start) / WINDOW_MS) * w;
        const down = ((k % 4) + 4) % 4 === 0;
        g.fillStyle = down ? accent : "rgba(255,255,255,0.55)";
        g.fillRect(x, 0, down ? 2 : 1, h);
      }
    }
    if (playerPosRef.current.trackId === track.id) {
      const x = ((playerPosRef.current.ms - start) / WINDOW_MS) * w;
      g.fillStyle = "#ff5c7a";
      g.fillRect(x, 0, 1, h);
    }
  }, [env, grid, start, track.id, playerPosRef]);

  const playing = nowPlaying?.id === track.id;
  const here = () => (playing && playerPosRef.current.trackId === track.id ? playerPosRef.current.ms : null);
  const set = (bpm: number, first: number) => act(() => api.gridSet(track.id, bpm, Math.max(0, first))).then(onChanged);
  const viewAtPlayhead = () => setCenter(here() ?? (grid ? grid.firstBeatMs + 2000 : 2000));
  const tapped = tapBpm(taps);

  return (
    <>
      <div className="section-title">{t("grid.title")}</div>
      {!grid ? (
        <div className="muted small">{t("grid.none")}</div>
      ) : (
        <div className="small">
          <span className="num">{grid.bpm.toFixed(2)} BPM</span> · {t("grid.firstBeat")} <span className="num">{(grid.firstBeatMs / 1000).toFixed(3)} s</span> ·{" "}
          <span className="src">{grid.source}</span>
        </div>
      )}
      <div className="row wrap">
        <button className="btn small ghost" onClick={viewAtPlayhead}>
          {t("grid.view")}
        </button>
        {grid && (
          <>
            <button className="btn small ghost" disabled={!playing} title={playing ? "" : t("cues.playFirst")} onClick={() => set(grid.bpm, ((here() ?? 0) % (4 * (60000 / grid.bpm))))}>
              {t("grid.downbeatHere")}
            </button>
            <button className="btn small ghost" onClick={() => set(grid.bpm, grid.firstBeatMs - 5)} title={t("grid.nudge")}>
              ◀ 5 ms
            </button>
            <button className="btn small ghost" onClick={() => set(grid.bpm, grid.firstBeatMs + 5)} title={t("grid.nudge")}>
              5 ms ▶
            </button>
          </>
        )}
      </div>
      {env && start != null && <canvas ref={canvas} className="grid-zoom" />}
      {grid && (
        <div className="row wrap">
          <input className="num" style={{ width: 80 }} value={bpmInput} onChange={(e) => setBpmInput(e.target.value)} aria-label="BPM" />
          <button
            className="btn small ghost"
            onClick={() => {
              const v = Number(bpmInput.replace(",", "."));
              if (v > 0) set(v, grid.firstBeatMs);
            }}
          >
            {t("grid.setBpm")}
          </button>
          <button className="btn small ghost" onClick={() => set(grid.bpm / 2, grid.firstBeatMs)}>
            ÷2
          </button>
          <button className="btn small ghost" onClick={() => set(grid.bpm * 2, grid.firstBeatMs)}>
            ×2
          </button>
          <button className="btn small ghost" onClick={() => setTaps((ts) => [...ts.filter((x) => performance.now() - x < 4000), performance.now()])}>
            {t("grid.tap")}
            {tapped ? ` ${tapped}` : taps.length ? ` (${taps.length})` : ""}
          </button>
          {tapped && (
            <button className="btn small" onClick={() => set(tapped, grid.firstBeatMs)}>
              {t("grid.useTap", { bpm: tapped })}
            </button>
          )}
          {grid.source === "user" && (
            <button className="link-btn" onClick={() => act(() => api.gridClear(track.id)).then(onChanged)}>
              {t("grid.reset")}
            </button>
          )}
        </div>
      )}
      {grid && (
        <div className="row wrap">
          <span className="muted small">{t("grid.loop")}</span>
          {[4, 8, 16].map((b) => (
            <button key={b} className="btn small ghost" disabled={!playing} onClick={() => act(() => api.gridLoop(track.id, here() ?? 0, b)).then(onChanged)}>
              {b}
            </button>
          ))}
        </div>
      )}
    </>
  );
}
