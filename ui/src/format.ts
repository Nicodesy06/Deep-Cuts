// Pure formatting helpers (unit-tested).

export function formatDuration(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "";
  const total = Math.round(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${m}:${String(s).padStart(2, "0")}`;
}

/** "12 h 05 min" style for long totals. */
export function formatLongDuration(ms: number): string {
  const totalMin = Math.round(ms / 60000);
  const d = Math.floor(totalMin / 1440);
  const h = Math.floor((totalMin % 1440) / 60);
  const m = totalMin % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return `${m}m`;
}

export function formatBpm(bpm: number | null | undefined): string {
  if (bpm == null) return "";
  return Number.isInteger(bpm) ? bpm.toFixed(0) : bpm.toFixed(2).replace(/0$/, "");
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

export function formatNumber(n: number, locale = "en"): string {
  return new Intl.NumberFormat(locale).format(n);
}

/** Hue for a Camelot code: adjacent wheel positions get adjacent hues. */
export function camelotHue(code: string | null | undefined): number | null {
  if (!code) return null;
  const m = /^(\d{1,2})([AB])$/.exec(code);
  if (!m) return null;
  const n = Number(m[1]);
  if (n < 1 || n > 12) return null;
  return ((n - 1) * 30 + 200) % 360;
}

export function trackLabel(t: { artist: string | null; title: string | null }): string {
  if (t.artist && t.title) return `${t.artist} — ${t.title}`;
  return t.title ?? t.artist ?? "Untitled";
}

export function sourceLabel(source: string): string {
  if (source.startsWith("import:")) return source.slice(7);
  return source;
}

/** Escape a value for use inside the query language. */
export function quoteQueryValue(v: string): string {
  return /[\s()"]/.test(v) ? `"${v.replace(/"/g, "")}"` : v;
}
