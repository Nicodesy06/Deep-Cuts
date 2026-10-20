// Preview → confirm → rename/move audio files from metadata. Undoable.
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type MovePlan, type RenameReport } from "../api";
import { useApp } from "../store";
import type { MsgKey } from "../i18n";

export const DEFAULT_PATTERN = "{artist} - {title} ({version})";
const TOKENS = ["artist", "title", "version", "remixer", "album", "label", "genre", "year", "bpm", "key", "camelot"];

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

export function RenameDialog({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [pattern, setPattern] = useState(DEFAULT_PATTERN);
  const [targetDir, setTargetDir] = useState<string | null>(null);
  const [plan, setPlan] = useState<MovePlan[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<RenameReport | null>(null);

  useEffect(() => {
    let live = true;
    const h = setTimeout(() => {
      api.planRename(ids, pattern, targetDir).then(
        (p) => live && (setPlan(p), setError(null)),
        (e) => live && (setPlan(null), setError(String(e))),
      );
    }, 250);
    return () => {
      live = false;
      clearTimeout(h);
    };
  }, [ids, pattern, targetDir]);

  const ready = plan?.filter((p) => !p.conflict) ?? [];

  const apply = async () => {
    setBusy(true);
    try {
      setReport(await api.applyRename(ready));
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
        <h3>{t("rename.title")}</h3>
        {!report && (
          <>
            <label className="field">
              <span>{t("rename.pattern")}</span>
              <input value={pattern} onChange={(e) => setPattern(e.target.value)} spellCheck={false} />
            </label>
            <p className="hint">{t("rename.tokens", { tokens: TOKENS.map((x) => `{${x}}`).join(" ") })}</p>
            <div className="row">
              <button
                className="btn ghost"
                onClick={async () => {
                  const r = await open({ directory: true, multiple: false, title: t("rename.chooseFolder") });
                  if (typeof r === "string") setTargetDir(r);
                }}
              >
                {t("rename.moveTo")}
              </button>
              <span className="path muted">{targetDir ?? t("rename.inPlace")}</span>
              {targetDir && (
                <button className="btn ghost" onClick={() => setTargetDir(null)}>
                  {t("rename.keepFolder")}
                </button>
              )}
            </div>
            {error && <div className="query-error">{error}</div>}
            {plan && (
              <>
                <p className="muted">{t("rename.summary", { ready: ready.length, total: plan.length })}</p>
                <div className="write-plan">
                  <table className="grid-table small">
                    <tbody>
                      {plan.slice(0, 300).map((p) => (
                        <tr key={p.fileId} className={p.conflict ? "muted" : ""}>
                          <td className="path">{fileName(p.from)}</td>
                          <td>{p.conflict ? <span className={p.conflict === "unchanged" ? "muted" : "err"}>{t(`rename.conflict.${p.conflict}` as MsgKey)}</span> : <>→ {targetDir ? p.to : fileName(p.to)}</>}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            )}
            <p className="hint">{t("rename.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn danger-solid" disabled={busy || ready.length === 0} onClick={apply}>
                {busy ? t("common.working") : t("rename.go", { n: ready.length })}
              </button>
            </div>
          </>
        )}
        {report && (
          <>
            <p className={report.failed.length ? "" : "ok"}>{t("rename.done", { moved: report.moved, failed: report.failed.length })}</p>
            {report.failed.length > 0 && (
              <ul className="small err">
                {report.failed.map((f) => (
                  <li key={f.path}>
                    {fileName(f.path)}: {f.error}
                  </li>
                ))}
              </ul>
            )}
            <p className="hint">{t("rename.undoHint")}</p>
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
