// Find & replace in metadata fields: preview, then one undoable step.
import { useEffect, useState } from "react";
import { api, type FindReplace, type ReplaceProposal } from "../api";
import { useApp } from "../store";

const FIELDS = ["title", "artist", "version", "remixer", "album", "label", "genre", "subgenre", "comment"];

export function FindReplaceDialog({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [spec, setSpec] = useState<FindReplace>({ fields: ["title", "artist"], find: "", replace: "", caseSensitive: false, wholeWord: false, regex: false });
  const [plan, setPlan] = useState<ReplaceProposal[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!spec.find || spec.fields.length === 0) {
      setPlan(null);
      setError(null);
      return;
    }
    let live = true;
    const h = setTimeout(() => {
      api.findReplacePreview(ids, spec).then(
        (p) => live && (setPlan(p), setError(null)),
        (e) => live && (setPlan(null), setError(String(e))),
      );
    }, 250);
    return () => {
      live = false;
      clearTimeout(h);
    };
  }, [ids, spec]);

  const set = (patch: Partial<FindReplace>) => setSpec((s) => ({ ...s, ...patch }));
  const apply = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      setDone(await api.findReplaceApply(plan));
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
        <h3>{t("fr.title", { n: ids.length })}</h3>
        {done == null ? (
          <>
            <div className="fr-grid">
              <label className="field">
                <span>{t("fr.find")}</span>
                <input autoFocus value={spec.find} onChange={(e) => set({ find: e.target.value })} spellCheck={false} />
              </label>
              <label className="field">
                <span>{t("fr.replace")}</span>
                <input value={spec.replace} onChange={(e) => set({ replace: e.target.value })} spellCheck={false} />
              </label>
            </div>
            <div className="chips">
              {FIELDS.map((f) => (
                <label key={f} className="chip-check">
                  <input
                    type="checkbox"
                    checked={spec.fields.includes(f)}
                    onChange={(e) => set({ fields: e.target.checked ? [...spec.fields, f] : spec.fields.filter((x) => x !== f) })}
                  />
                  {t(`col.${f}` as never)}
                </label>
              ))}
            </div>
            <div className="row">
              <label className="toggle">
                <input type="checkbox" checked={spec.caseSensitive} onChange={(e) => set({ caseSensitive: e.target.checked })} />
                {t("fr.case")}
              </label>
              <label className="toggle">
                <input type="checkbox" checked={spec.wholeWord} onChange={(e) => set({ wholeWord: e.target.checked })} />
                {t("fr.word")}
              </label>
              <label className="toggle">
                <input type="checkbox" checked={spec.regex} onChange={(e) => set({ regex: e.target.checked })} />
                {t("fr.regex")}
              </label>
            </div>
            {error && <div className="query-error">{error}</div>}
            {plan && (
              <>
                <p className="muted">{t("fr.summary", { n: plan.length })}</p>
                {plan.length > 0 && (
                  <div className="write-plan">
                    <table className="grid-table small">
                      <tbody>
                        {plan.slice(0, 300).map((p) => (
                          <tr key={p.trackId + p.field}>
                            <td className="muted">{t(`col.${p.field}` as never)}</td>
                            <td className="muted">{p.old}</td>
                            <td>→ {p.new ?? "∅"}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            )}
            <p className="hint">{t("fr.safety")}</p>
            <div className="row end">
              <button className="btn ghost" onClick={onClose}>
                {t("action.cancel")}
              </button>
              <button className="btn" disabled={busy || !plan || plan.length === 0} onClick={apply}>
                {t("fr.go", { n: plan?.length ?? 0 })}
              </button>
            </div>
          </>
        ) : (
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
