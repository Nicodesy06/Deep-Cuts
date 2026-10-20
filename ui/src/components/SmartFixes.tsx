import { useEffect, useMemo, useState } from "react";
import { api, type FixProposal } from "../api";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

const DEFAULT_RULES = ["unicode", "urls", "trackNumber", "featuring", "separators", "caps", "genre", "whitespace", "version", "remixer"];

export function SmartFixes() {
  const { t, bump } = useApp();
  const act = useAction();
  const [rules, setRules] = useState<string[]>(DEFAULT_RULES);
  const [available, setAvailable] = useState<{ id: string; fields: string[] }[]>([]);
  const [scope, setScope] = useState("");
  const [busy, setBusy] = useState(false);
  const [props, setProps] = useState<FixProposal[] | null>(null);
  const [rejected, setRejected] = useState<Set<number>>(new Set());

  useEffect(() => {
    api.fixRules().then(setAvailable, () => {});
  }, []);

  const preview = async () => {
    setBusy(true);
    const r = await act(() => api.previewFixes({ query: scope }, rules, 2000));
    setBusy(false);
    if (r) {
      setProps(r);
      setRejected(new Set());
    }
  };

  const accepted = useMemo(() => (props ?? []).filter((_, i) => !rejected.has(i)), [props, rejected]);

  const apply = async () => {
    const n = await act(
      () => api.applyFixes(accepted.map((p) => ({ trackId: p.trackId, field: p.field, newValue: p.newValue }))),
      t("fixes.applied", { n: accepted.length }),
    );
    if (n !== undefined) {
      setProps(null);
      bump();
    }
  };

  return (
    <section className="panel">
      <h2>{t("fixes.title")}</h2>
      <p className="hint">{t("fixes.hint")}</p>
      <div className="chips fix-rules">
        {available.map((r) => (
          <label key={r.id} className="chip toggle" title={r.fields.join(", ")}>
            <input
              type="checkbox"
              checked={rules.includes(r.id)}
              onChange={(e) => setRules((rs) => (e.target.checked ? [...rs, r.id] : rs.filter((x) => x !== r.id)))}
            />
            {t(`fixes.rule.${r.id}` as MsgKey)}
          </label>
        ))}
      </div>
      <div className="row wrap mt">
        <input className="mono grow" value={scope} onChange={(e) => setScope(e.target.value)} placeholder={t("fixes.scope")} spellCheck={false} />
        <button className="btn" disabled={busy || rules.length === 0} onClick={preview}>
          {busy ? t("common.working") : t("fixes.preview")}
        </button>
      </div>
      {props && (
        <>
          <div className="row space mt">
            <span className="muted">{props.length === 0 ? t("fixes.none") : t("fixes.found", { n: props.length, accepted: accepted.length })}</span>
            {props.length > 0 && (
              <div className="row">
                <button className="btn small ghost" onClick={() => setRejected(new Set())}>
                  {t("fixes.all")}
                </button>
                <button className="btn small ghost" onClick={() => setRejected(new Set(props.map((_, i) => i)))}>
                  {t("fixes.noneSel")}
                </button>
                <button className="btn small" disabled={accepted.length === 0} onClick={apply}>
                  {t("fixes.apply", { n: accepted.length })}
                </button>
              </div>
            )}
          </div>
          {props.length > 0 && (
            <div className="fix-table">
              <table className="grid-table small">
                <tbody>
                  {props.map((p, i) => (
                    <tr key={i} className={rejected.has(i) ? "rejected" : ""}>
                      <td>
                        <input
                          type="checkbox"
                          checked={!rejected.has(i)}
                          onChange={() =>
                            setRejected((r) => {
                              const n = new Set(r);
                              if (n.has(i)) n.delete(i);
                              else n.add(i);
                              return n;
                            })
                          }
                        />
                      </td>
                      <td className="muted">{p.artist}</td>
                      <td>{p.field}</td>
                      <td className="del">{p.oldValue ?? "∅"}</td>
                      <td className="ins">{p.newValue ?? "∅"}</td>
                      <td className="muted">{p.rules.map((r) => t(`fixes.rule.${r}` as MsgKey)).join(", ")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </section>
  );
}
