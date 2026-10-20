// Import metadata from a CSV: map columns to fields, preview, apply (undoable).
import { useEffect, useMemo, useState } from "react";
import { api, type CsvMapping, type CsvPreview, type CsvSheet } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

const TARGETS = ["title", "artist", "version", "remixer", "album", "label", "genre", "subgenre", "year", "bpm", "key", "comment", "rating", "isrc", "decision"];

export function CsvImportDialog({ path, onClose }: { path: string; onClose: () => void }) {
  const { t, bump } = useApp();
  const [sheet, setSheet] = useState<CsvSheet | null>(null);
  const [mapping, setMapping] = useState<CsvMapping | null>(null);
  const [preview, setPreview] = useState<CsvPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.csvRead(path).then(
      (s) => {
        setSheet(s);
        setMapping(s.suggested ?? { matchBy: "artistTitle", keyColumn: 0, titleColumn: 1, fields: [] });
      },
      (e) => setError(String(e)),
    );
  }, [path]);

  useEffect(() => {
    if (!mapping) return;
    setPreview(null);
    api.csvPreview(path, mapping).then(
      (p) => (setPreview(p), setError(null)),
      (e) => setError(String(e)),
    );
  }, [path, mapping]);

  const fieldOf = useMemo(() => new Map(mapping?.fields ?? []), [mapping]);
  const setField = (col: number, field: string) => {
    if (!mapping) return;
    const rest = mapping.fields.filter(([c]) => c !== col);
    setMapping({ ...mapping, fields: field ? [...rest, [col, field]] : rest });
  };
  const apply = async () => {
    if (!preview) return;
    setBusy(true);
    try {
      setDone(await api.csvApply(preview.proposals));
      bump();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal wide" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t("csv.title")}</h3>
        <p className="muted small path">{path}</p>
        {error && <div className="query-error">{error}</div>}
        {sheet && mapping && done == null && (
          <>
            <div className="row wrap">
              <label className="field">
                <span>{t("csv.matchBy")}</span>
                <select value={mapping.matchBy} onChange={(e) => setMapping({ ...mapping, matchBy: e.target.value as CsvMapping["matchBy"] })}>
                  <option value="path">{t("csv.match.path")}</option>
                  <option value="isrc">ISRC</option>
                  <option value="artistTitle">{t("csv.match.artistTitle")}</option>
                </select>
              </label>
              <label className="field">
                <span>{mapping.matchBy === "artistTitle" ? t("col.artist") : t("csv.keyColumn")}</span>
                <select value={mapping.keyColumn} onChange={(e) => setMapping({ ...mapping, keyColumn: Number(e.target.value) })}>
                  {sheet.headers.map((h, i) => (
                    <option key={i} value={i}>
                      {h}
                    </option>
                  ))}
                </select>
              </label>
              {mapping.matchBy === "artistTitle" && (
                <label className="field">
                  <span>{t("col.title")}</span>
                  <select value={mapping.titleColumn ?? 0} onChange={(e) => setMapping({ ...mapping, titleColumn: Number(e.target.value) })}>
                    {sheet.headers.map((h, i) => (
                      <option key={i} value={i}>
                        {h}
                      </option>
                    ))}
                  </select>
                </label>
              )}
            </div>
            <div className="write-plan">
              <table className="grid-table small">
                <thead>
                  <tr>
                    <th>{t("csv.column")}</th>
                    <th>{t("csv.example")}</th>
                    <th>{t("csv.field")}</th>
                  </tr>
                </thead>
                <tbody>
                  {sheet.headers.map((h, i) => (
                    <tr key={i}>
                      <td>{h}</td>
                      <td className="muted">{sheet.rows.find((r) => r[i])?.[i] ?? ""}</td>
                      <td>
                        <select value={fieldOf.get(i) ?? ""} onChange={(e) => setField(i, e.target.value)}>
                          <option value="">{t("csv.ignore")}</option>
                          {TARGETS.map((f) => (
                            <option key={f} value={f}>
                              {f === "decision" ? t("csv.decision") : f === "isrc" ? "ISRC" : t(`col.${f}` as MsgKey)}
                            </option>
                          ))}
                        </select>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {preview ? (
              <>
                <p>
                  {t("csv.summary", { rows: preview.rows, matched: preview.matched, unmatched: preview.unmatched, ambiguous: preview.ambiguous, changes: preview.proposals.length })}
                </p>
                {preview.proposals.length > 0 && (
                  <details>
                    <summary>{t("csv.showChanges")}</summary>
                    <div className="write-plan">
                      <table className="grid-table small">
                        <tbody>
                          {preview.proposals.slice(0, 300).map((p) => (
                            <tr key={p.trackId + p.field}>
                              <td className="muted">{p.field === "decision" ? t("csv.decision") : p.field === "isrc" ? "ISRC" : t(`col.${p.field}` as MsgKey)}</td>
                              <td className="muted">{p.old || "∅"}</td>
                              <td>→ {p.new ?? "∅"}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </details>
                )}
                {preview.invalid.length > 0 && <p className="hint err">{t("csv.invalid", { n: preview.invalid.length, examples: preview.invalid.slice(0, 5).join(", ") })}</p>}
              </>
            ) : (
              <p className="muted">{t("common.loading")}</p>
            )}
            <p className="hint">{t("csv.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn" disabled={busy || !preview || preview.proposals.length === 0} onClick={apply}>
                {t("csv.go", { n: preview?.proposals.length ?? 0 })}
              </button>
            </div>
          </>
        )}
        {done != null && (
          <>
            <p className="ok">{t("fr.done", { n: done })}</p>
            <p className="hint">{t("fr.undoHint")}</p>
            <div className="row end">
              <button className="btn" onClick={onClose}>
                {t("action.ok")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
