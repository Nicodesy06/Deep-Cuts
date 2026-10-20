// Canvas waveform from the analyser's per-bin data (peak, rms, low, high).
import { useEffect, useRef } from "react";

export interface Marker {
  ms: number;
  color: string;
  label?: string;
}

interface Props {
  data: number[] | null;
  durationMs: number;
  positionMs: number;
  markers?: Marker[];
  height?: number;
  onSeek?: (ms: number) => void;
}

export function Waveform({ data, durationMs, positionMs, markers = [], height = 56, onSeek }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = Math.max(1, Math.floor(w * dpr));
    c.height = Math.max(1, Math.floor(h * dpr));
    const g = c.getContext("2d");
    if (!g) return;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, w, h);
    const css = getComputedStyle(c);
    const played = css.getPropertyValue("--wf-played").trim() || "#c6ff3d";
    const rest = css.getPropertyValue("--wf-rest").trim() || "#5b6170";
    const high = css.getPropertyValue("--wf-high").trim() || "#58d6ff";
    const progress = durationMs > 0 ? Math.min(1, positionMs / durationMs) : 0;
    const mid = h / 2;
    if (data && data.length >= 4) {
      const bins = data.length / 4;
      const cols = Math.min(bins, Math.floor(w / 2));
      for (let x = 0; x < cols; x++) {
        const b0 = Math.floor((x * bins) / cols);
        const b1 = Math.max(b0 + 1, Math.floor(((x + 1) * bins) / cols));
        let peak = 0,
          rms = 0,
          hi = 0,
          lo = 0;
        for (let b = b0; b < b1; b++) {
          peak = Math.max(peak, data[b * 4]);
          rms = Math.max(rms, data[b * 4 + 1]);
          lo += data[b * 4 + 2];
          hi += data[b * 4 + 3];
        }
        const px = (x / cols) * w;
        const bw = Math.max(1, w / cols - 1);
        const isPlayed = x / cols < progress;
        const ph = (peak / 255) * (h / 2 - 2);
        const rh = (rms / 255) * (h / 2 - 2);
        g.globalAlpha = isPlayed ? 0.45 : 0.3;
        g.fillStyle = hi > lo * 1.2 ? high : isPlayed ? played : rest;
        g.fillRect(px, mid - ph, bw, ph * 2);
        g.globalAlpha = 1;
        g.fillStyle = isPlayed ? played : rest;
        g.fillRect(px, mid - rh, bw, rh * 2);
      }
    } else {
      g.fillStyle = rest;
      g.fillRect(0, mid - 1, w, 2);
      g.fillStyle = played;
      g.fillRect(0, mid - 1, w * progress, 2);
    }
    for (const m of markers) {
      if (durationMs <= 0) continue;
      const x = (m.ms / durationMs) * w;
      g.fillStyle = m.color;
      g.fillRect(x, 0, 2, h);
      g.beginPath();
      g.moveTo(x - 4, 0);
      g.lineTo(x + 6, 0);
      g.lineTo(x + 1, 6);
      g.fill();
    }
    g.fillStyle = "#ffffff";
    g.fillRect(progress * w, 0, 1.5, h);
  }, [data, durationMs, positionMs, markers]);

  return (
    <canvas
      ref={canvas}
      className="waveform"
      style={{ height }}
      onMouseDown={(e) => {
        if (!onSeek || durationMs <= 0) return;
        const r = e.currentTarget.getBoundingClientRect();
        onSeek(((e.clientX - r.left) / r.width) * durationMs);
      }}
      role="slider"
      aria-valuemin={0}
      aria-valuemax={durationMs}
      aria-valuenow={positionMs}
    />
  );
}
