// Preview → confirm → write tags into audio files, with optional backup copies.
import { useEffect, useState } from "react";
import { api, type WritePlan, type WriteReport } from "../api";
import { useApp } from "../store";

export function WriteTagsDialog({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [plan, setPlan] = useState<WritePlan[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [keepBackup, setKeepBackup] = useState(true);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<WriteReport | null>(null);

  useEffect(() => {
    api.planTagWrite(ids).then(setPlan, (e) => setError(String(e)));
  }, [ids]);

  const write = async () => {
    setBusy(true);
    try {
      const r = await api.writeTags(plan!.map((p) => p.trackId), keepBackup);
      setReport(r);
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
        <h3>{t("write.title")}</h3>
        {error && <div className="query-error">{error}</div>}
        {!plan && !error && <p className="muted">{t("common.loading")}</p>}
        {plan && !report && (
          <>
            <p className="muted">{plan.length === 0 ? t("write.nothing") : t("write.summary", { files: plan.length, tracks: ids.length })}</p>
            {plan.length > 0 && (
              <div className="write-plan">
                <table className="grid-table small">
                  <tbody>
                    {plan.slice(0, 200).flatMap((p) =>
                      p.changes.map((c, i) => (
                        <tr key={p.trackId + c.field}>
                          <td className="path muted">{i === 0 ? p.path.split(/[\\/]/).pop() : ""}</td>
                          <td>{c.field}</td>
                          <td className="muted">{c.fileValue ?? "∅"}</td>
                          <td>→ {c.newValue ?? "∅"}</td>
                        </tr>
                      )),
                    )}
                  </tbody>
                </table>
              </div>
            )}
            <label className="toggle">
              <input type="checkbox" checked={keepBackup} onChange={(e) => setKeepBackup(e.target.checked)} />
              {t("write.keepBackup")}
            </label>
            <p className="hint">{t("write.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn danger-solid" disabled={busy || plan.length === 0} onClick={write}>
                {busy ? t("common.working") : t("write.go", { n: plan.length })}
              </button>
            </div>
          </>
        )}
        {report && (
          <>
            <p className={report.failed.length ? "" : "ok"}>{t("write.done", { written: report.written, failed: report.failed.length })}</p>
            {report.backupDir && <p className="hint path">{t("write.backupAt", { dir: report.backupDir })}</p>}
            {report.failed.length > 0 && (
              <ul className="small err">
                {report.failed.map((f) => (
                  <li key={f.trackId}>
                    {f.path.split(/[\\/]/).pop()}: {f.error}
                  </li>
                ))}
              </ul>
            )}
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
