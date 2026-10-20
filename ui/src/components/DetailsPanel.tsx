// Right-hand inspector: metadata with provenance, decisions, notes,
// measured analysis, cues. Multi-selection switches to bulk editing.
import { useEffect, useState } from "react";
import { api, type Decision, type TrackDetail, type TrackRow } from "../api";
import { formatBpm, formatDuration, sourceLabel } from "../format";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";
import { CueEditor, HistorySection, TagsSection } from "./TrackExtras";
import { KeyChip } from "./TrackTable";

const EDITABLE: { field: string; label: MsgKey }[] = [
  { field: "title", label: "col.title" },
  { field: "artist", label: "col.artist" },
  { field: "version", label: "col.version" },
  { field: "remixer", label: "col.remixer" },
  { field: "album", label: "col.album" },
  { field: "label", label: "col.label" },
  { field: "genre", label: "col.genre" },
  { field: "subgenre", label: "col.subgenre" },
  { field: "year", label: "col.year" },
  { field: "bpm", label: "col.bpm" },
  { field: "key", label: "col.key" },
  { field: "comment", label: "col.comment" },
];

function effective(d: TrackDetail, field: string): { value: string | null; source: string | null; userOverride: boolean } {
  const rank = (s: string) => (s === "user" ? 0 : s.startsWith("import:") ? 1 : s === "file" ? 2 : s === "analysis" ? 3 : 4);
  const vals = d.fields.filter((f) => f.field === field).sort((a, b) => rank(a.source) - rank(b.source));
  const top = vals[0];
  return { value: top?.value ?? null, source: top?.source ?? null, userOverride: vals.some((v) => v.source === "user") };
}

function FieldRow({
  label,
  value,
  source,
  others,
  userOverride,
  onSave,
  onRevert,
}: {
  label: string;
  value: string | null;
  source: string | null;
  others: string;
  userOverride: boolean;
  onSave: (v: string | null) => void;
  onRevert: () => void;
}) {
  const [draft, setDraft] = useState(value ?? "");
  useEffect(() => setDraft(value ?? ""), [value]);
  const commit = () => {
    if ((value ?? "") !== draft) onSave(draft.trim() === "" ? null : draft);
  };
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span className="field-input">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setDraft(value ?? "");
          }}
        />
        {source && (
          <span className={`src src-${source.split(":")[0]}`} title={others}>
            {sourceLabel(source)}
          </span>
        )}
        {userOverride && (
          <button className="link-btn" onClick={(e) => (e.preventDefault(), onRevert())} title={others}>
            ↺
          </button>
        )}
      </span>
    </label>
  );
}

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="metric" title={hint}>
      <span className="metric-label">{label}</span>
      <span className="metric-value num">{value}</span>
    </div>
  );
}

function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return null;
  const min = Math.max(-60, Math.min(...values));
  const max = Math.max(...values);
  const w = 280;
  const h = 44;
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * w},${h - ((Math.max(v, min) - min) / Math.max(1, max - min)) * (h - 4) - 2}`);
  return (
    <svg className="sparkline" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" role="img" aria-label="energy curve">
      <polyline points={pts.join(" ")} fill="none" strokeWidth="2" />
    </svg>
  );
}

export function DetailsPanel({ selected, onChanged }: { selected: TrackRow[]; onChanged: () => void }) {
  const { t, dataVersion, play } = useApp();
  const act = useAction();
  const [detail, setDetail] = useState<TrackDetail | null>(null);
  const [notes, setNotes] = useState("");
  const [bulkField, setBulkField] = useState("genre");
  const [bulkValue, setBulkValue] = useState("");
  const one = selected.length === 1 ? selected[0] : null;

  useEffect(() => {
    if (!one) return setDetail(null);
    let alive = true;
    api.trackDetail(one.id).then((d) => {
      if (!alive) return;
      setDetail(d);
      setNotes(d.notes ?? "");
    }, () => alive && setDetail(null));
    return () => {
      alive = false;
    };
  }, [one?.id, dataVersion]);

  const ids = selected.map((s) => s.id);
  const decide = (d: Decision | null) => act(() => api.setDecision(ids, d)).then(onChanged);

  if (selected.length === 0) {
    return (
      <aside className="details">
        <div className="details-empty muted">{t("details.none")}</div>
      </aside>
    );
  }

  const decisionBar = (current: Decision | null | undefined) => (
    <div className="seg decisions wide">
      {(["keep", "maybe", "pass"] as Decision[]).map((d) => (
        <button key={d} className={`dec-btn dec-${d} ${current === d ? "active" : ""}`} onClick={() => decide(current === d ? null : d)}>
          {t(`decision.${d}` as MsgKey)}
        </button>
      ))}
    </div>
  );

  if (!one) {
    return (
      <aside className="details">
        <h3>{t("details.multi", { n: selected.length })}</h3>
        {decisionBar(null)}
        <div className="bulk">
          <span className="section-title">{t("details.bulkEdit")}</span>
          <select value={bulkField} onChange={(e) => setBulkField(e.target.value)}>
            {[...EDITABLE, { field: "rating", label: "col.rating" as MsgKey }].map((f) => (
              <option key={f.field} value={f.field}>
                {t(f.label)}
              </option>
            ))}
          </select>
          <input value={bulkValue} onChange={(e) => setBulkValue(e.target.value)} placeholder={t("details.bulkValue")} />
          <div className="row">
            <button
              className="btn"
              onClick={() =>
                act(() => api.editField(ids, bulkField, bulkValue.trim() || null), t("details.bulkDone", { n: ids.length })).then(onChanged)
              }
            >
              {t("details.apply")}
            </button>
            <button className="btn ghost" onClick={() => act(() => api.revertField(ids, bulkField)).then(onChanged)}>
              {t("details.revertAll")}
            </button>
          </div>
          <p className="hint">{t("details.bulkHint")}</p>
        </div>
      </aside>
    );
  }

  const d = detail;
  const a = d?.analysis;
  const tr = d?.track ?? one;
  return (
    <aside className="details">
      <div className="details-head">
        <button className="icon-btn play-sm" onClick={() => play(tr, tr.previewMs ?? 0)} aria-label={t("player.play")}>
          ▶
        </button>
        <div>
          <div className="dh-title">{tr.title ?? t("details.untitled")}</div>
          <div className="dh-artist muted">{tr.artist}</div>
        </div>
      </div>
      <div className="dh-chips">
        <span className="num chip">{formatBpm(tr.bpm) || "—"} BPM</span>
        <KeyChip t={tr} />
        <span className="num chip">{formatDuration(tr.durationMs)}</span>
        {tr.format && <span className="chip">{tr.format.toUpperCase()}</span>}
        {tr.missing && <span className="badge badge-warn">{t("details.missing")}</span>}
      </div>
      {decisionBar(tr.decision)}
      <div className="row space">
        <span className="rating-input">
          {[1, 2, 3, 4, 5].map((n) => (
            <button
              key={n}
              className={(tr.rating ?? 0) >= n ? "on" : ""}
              onClick={() => act(() => api.editField([tr.id], "rating", String(tr.rating === n ? 0 : n))).then(onChanged)}
            >
              ★
            </button>
          ))}
        </span>
        <button className={`link-btn ${tr.favorite ? "fav" : ""}`} onClick={() => act(() => api.setFavorite([tr.id], !tr.favorite)).then(onChanged)}>
          {tr.favorite ? "♥" : "♡"} {t("details.favorite")}
        </button>
      </div>
      {d && Object.keys(d.contexts).length > 0 && (
        <div className="hint">
          {t("details.contexts")}: {Object.entries(d.contexts).map(([c, v]) => `${c}: ${v}`).join(" · ")}
        </div>
      )}

      {d && <TagsSection d={d} onChanged={onChanged} />}

      <div className="section-title">{t("details.metadata")}</div>
      {d &&
        EDITABLE.map((f) => {
          const eff = effective(d, f.field);
          const others = d.fields
            .filter((x) => x.field === f.field)
            .map((x) => `${sourceLabel(x.source)}: ${x.value ?? "∅"}`)
            .join("\n");
          return (
            <FieldRow
              key={f.field}
              label={t(f.label)}
              value={eff.value}
              source={eff.source}
              others={others}
              userOverride={eff.userOverride}
              onSave={(v) => act(() => api.editField([tr.id], f.field, v)).then(onChanged)}
              onRevert={() => act(() => api.revertField([tr.id], f.field)).then(onChanged)}
            />
          );
        })}
      <label className="field col">
        <span className="field-label">{t("details.notes")}</span>
        <textarea
          rows={3}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          onBlur={() => notes !== (d?.notes ?? "") && act(() => api.setNotes(tr.id, notes)).then(onChanged)}
        />
      </label>

      <div className="section-title">
        {t("details.analysis")}
        {a && <span className="muted small"> · {a.analyzer} {a.analyzerVersion}</span>}
      </div>
      {!a ? (
        <div className="row">
          <span className="muted">{tr.analysisStatus === "queued" ? t("details.queued") : tr.analysisStatus === "failed" ? t("details.failed") : t("details.notAnalyzed")}</span>
          <button className="btn small" onClick={() => act(() => api.enqueueAnalysis([tr.id], true), t("details.queuedToast")).then(onChanged)}>
            {t("action.analyze")}
          </button>
        </div>
      ) : (
        <>
          <div className="metrics">
            <Metric label={t("an.bpm")} value={a.tempo ? a.tempo.bpm.toFixed(2) : "—"} hint={t("an.bpmHint")} />
            <Metric label={t("an.periodicity")} value={a.tempo ? a.tempo.periodicity.toFixed(2) : "—"} hint={t("an.periodicityHint")} />
            <Metric label={t("an.key")} value={a.key ? `${a.key.camelot} · ${a.key.name}` : "—"} hint={t("an.keyHint")} />
            <Metric label={t("an.keyCorr")} value={a.key ? `${a.key.correlation.toFixed(2)} (Δ${a.key.margin.toFixed(2)})` : "—"} hint={t("an.keyCorrHint")} />
            <Metric label="LUFS" value={a.loudness.integratedLufs?.toFixed(1) ?? "—"} hint={t("an.lufsHint")} />
            <Metric label={t("an.peak")} value={`${a.loudness.samplePeakDb.toFixed(1)} dB`} />
            <Metric label={t("an.crest")} value={a.loudness.crestDb != null ? `${a.loudness.crestDb.toFixed(1)} dB` : "—"} hint={t("an.crestHint")} />
            <Metric label={t("an.spread")} value={a.loudness.blockSpreadLu != null ? `${a.loudness.blockSpreadLu.toFixed(1)} LU` : "—"} hint={t("an.spreadHint")} />
            <Metric label={t("an.centroid")} value={`${Math.round(a.spectral.centroidHz)} Hz`} hint={t("an.centroidHint")} />
            <Metric label={t("an.low")} value={`${Math.round(a.spectral.lowEnergyRatio * 100)}%`} hint={t("an.lowHint")} />
            <Metric label={t("an.onsets")} value={`${a.spectral.onsetRate.toFixed(2)}/s`} hint={t("an.onsetsHint")} />
            <Metric label={t("an.firstBeat")} value={a.tempo ? `${(a.tempo.firstBeatMs / 1000).toFixed(3)} s` : "—"} />
            <Metric
              label={t("an.bandwidth")}
              value={!a.bandwidth ? "—" : a.bandwidth.cutoffHz != null ? `${(a.bandwidth.cutoffHz / 1000).toFixed(1)} kHz` : t("an.fullBand")}
              hint={t("an.bandwidthHint")}
            />
          </div>
          <div className="section-sub muted">{t("an.energyCurve")}</div>
          <Sparkline values={a.energyCurveDb} />
          <p className="hint">{t("an.disclaimer")}</p>
          <button className="link-btn" onClick={() => act(() => api.enqueueAnalysis([tr.id], true), t("details.queuedToast")).then(onChanged)}>
            {t("action.reanalyze")}
          </button>
        </>
      )}

      {d && <CueEditor track={tr} cues={d.cues} onChanged={onChanged} />}
      {d && d.playlists.length > 0 && (
        <>
          <div className="section-title">{t("details.playlists")}</div>
          <div className="chips">
            {d.playlists.map((p) => (
              <span key={p.id} className="chip">
                {p.name}
              </span>
            ))}
          </div>
        </>
      )}
      <div className="section-title">{t("details.file")}</div>
      <div className="path small muted" title={tr.path ?? ""}>
        {tr.path}
      </div>
      <div className="small muted">
        {tr.bitrate ? `${tr.bitrate} kbps · ` : ""}
        {t("details.added")} {new Date(tr.createdAt).toLocaleString()}
      </div>
      {d && <HistorySection d={d} />}
    </aside>
  );
}
