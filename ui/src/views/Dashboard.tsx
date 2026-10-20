import { useEffect, useState } from "react";
import { api, type Bucket, type LibraryStats } from "../api";
import { importFolders } from "../actions";
import { BarChart } from "../components/BarChart";
import { formatBytes, formatLongDuration, formatNumber } from "../format";
import { useApp } from "../store";

/** Numeric histogram buckets with the empty ones filled in, so the axis is continuous. */
export function fillBuckets(data: Bucket[], step: number): Bucket[] {
  const nums = data.map((d) => Number(d.label)).filter((n) => Number.isFinite(n));
  if (nums.length !== data.length || nums.length === 0) return data;
  const by = new Map(data.map((d) => [Number(d.label), d.count]));
  const out: Bucket[] = [];
  for (let v = Math.min(...nums); v <= Math.max(...nums); v += step) out.push({ label: String(v), count: by.get(v) ?? 0 });
  return out;
}

function Tile({ label, value, sub, onClick }: { label: string; value: string; sub?: string; onClick?: () => void }) {
  const El = onClick ? "button" : "div";
  return (
    <El className={`tile ${onClick ? "tile-link" : ""}`} onClick={onClick}>
      <span className="tile-label">{label}</span>
      <span className="tile-value num">{value}</span>
      {sub && <span className="tile-sub muted">{sub}</span>}
    </El>
  );
}

export function Dashboard() {
  const { t, lang, dataVersion, openLibrary, go, toast, bump } = useApp();
  const [s, setS] = useState<LibraryStats | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    api.stats().then(setS, (e) => setErr(String(e)));
  }, [dataVersion]);

  if (err) return <div className="page"><div className="query-error">{err}</div></div>;
  if (!s) return <div className="page muted">{t("common.loading")}</div>;
  const n = (v: number) => formatNumber(v, lang);
  const pct = (v: number) => (s.totalTracks ? `${Math.round((v / s.totalTracks) * 100)}%` : "—");

  if (s.totalTracks === 0) {
    return (
      <div className="page">
        <div className="hero">
          <h1>{t("dash.welcome")}</h1>
          <p className="muted">{t("dash.welcomeBody")}</p>
          <div className="row">
            <button className="btn big" onClick={() => importFolders(t, toast, bump)}>
              {t("action.importFolder")}
            </button>
            <button className="btn ghost big" onClick={() => go("sync")}>
              {t("dash.importRekordbox")}
            </button>
          </div>
          <ul className="hero-points">
            <li>{t("dash.point1")}</li>
            <li>{t("dash.point2")}</li>
            <li>{t("dash.point3")}</li>
          </ul>
        </div>
      </div>
    );
  }

  return (
    <div className="page dashboard">
      <header className="page-head">
        <h1>{t("nav.dashboard")}</h1>
        <span className="muted">{t("dash.subtitle")}</span>
      </header>
      <div className="tiles">
        <Tile label={t("dash.tracks")} value={n(s.totalTracks)} sub={`+${n(s.addedLast7Days)} ${t("dash.last7")}`} onClick={() => openLibrary({})} />
        <Tile label={t("dash.duration")} value={formatLongDuration(s.totalDurationMs)} sub={formatBytes(s.totalSizeBytes)} />
        <Tile label={t("dash.analyzed")} value={pct(s.analyzed)} sub={`${n(s.analyzed)} / ${n(s.totalTracks)}`} onClick={() => go("queue")} />
        <Tile label={t("decision.keep")} value={n(s.keep)} sub={pct(s.keep)} onClick={() => openLibrary({ query: "decision:keep" })} />
        <Tile label={t("decision.maybe")} value={n(s.maybe)} sub={pct(s.maybe)} onClick={() => openLibrary({ query: "decision:maybe" })} />
        <Tile label={t("decision.pass")} value={n(s.pass)} sub={pct(s.pass)} onClick={() => openLibrary({ query: "decision:pass" })} />
        <Tile label={t("dash.undecided")} value={n(s.undecided)} onClick={() => openLibrary({ query: "decision:none" })} />
        <Tile label={t("dash.missing")} value={n(s.missingFiles)} onClick={() => openLibrary({ query: "missing:yes" })} />
        <Tile label={t("dash.duplicates")} value={n(s.duplicateHashGroups)} sub={t("dash.identicalFiles")} onClick={() => go("health")} />
        <Tile label={t("dash.playlists")} value={n(s.playlists)} sub={`${n(s.notInPlaylist)} ${t("dash.notInPlaylist")}`} />
      </div>
      <div className="charts">
        <BarChart title={t("dash.bpmDist")} data={fillBuckets(s.bpm, 2)} emptyText={t("dash.noData")} onSelect={(b) => openLibrary({ query: `bpm:${b.label}-${Number(b.label) + 1.99}` })} />
        <BarChart title={t("dash.keyDist")} data={s.keys} emptyText={t("dash.noData")} onSelect={(b) => openLibrary({ query: `key:${b.label}` })} />
        <BarChart title={t("dash.genres")} data={s.genres} horizontal emptyText={t("dash.noData")} onSelect={(b) => openLibrary({ query: `genre:"${b.label}"` })} />
        <BarChart title={t("dash.loudness")} data={fillBuckets(s.loudness, 1)} unit=" LUFS" emptyText={t("dash.noLoudness")} onSelect={(b) => openLibrary({ query: `lufs:${b.label}-${Number(b.label) + 0.99}` })} />
        <BarChart title={t("dash.growth")} data={s.growth} emptyText={t("dash.noData")} />
        <BarChart title={t("dash.formats")} data={s.formats} horizontal emptyText={t("dash.noData")} onSelect={(b) => openLibrary({ query: `format:${b.label.toLowerCase()}` })} />
      </div>
      <section className="panel">
        <h2>{t("dash.gaps")}</h2>
        <div className="gap-list">
          {[
            ["dash.noGenre", s.withoutGenre, "genre:none"],
            ["dash.noBpm", s.withoutBpm, "bpm:none"],
            ["dash.noKey", s.withoutKey, "key:none"],
            ["dash.noArtwork", s.withoutArtwork, "artwork:no"],
            ["dash.unrated", s.unrated, "rating:0"],
            ["dash.failed", s.analysisFailed, ""],
          ].map(([k, v, q]) => (
            <button key={k as string} className="gap" onClick={() => (q ? openLibrary({ query: q as string }) : go("queue"))}>
              <span>{t(k as never)}</span>
              <span className="num">{n(v as number)}</span>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
