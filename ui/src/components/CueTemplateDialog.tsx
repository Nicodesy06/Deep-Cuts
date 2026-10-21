// Cue templates: hot cues, memory cues and loops placed on analysed sections
// (first beat, main, breakdown, drop, outro, end) with bar offsets, snapped
// to the beat grid. Applying is one undoable step.
import { useEffect, useState } from "react";
import { api, type CueAnchor, type CueTemplate, type CueTemplateSlot } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

const ANCHORS: CueAnchor[] = ["firstBeat", "main", "breakdown", "drop", "outro", "end"];
const LETTERS = "ABCDEFGH";

const blank = (): CueTemplate => ({
  id: "",
  name: "",
  replaceHot: false,
  slots: [
    { anchor: "firstBeat", occurrence: 1, offsetBars: 0, kind: "hotcue", hot: 0, loopBars: null, name: "Start", color: null },
    { anchor: "breakdown", occurrence: 1, offsetBars: -8, kind: "hotcue", hot: 1, loopBars: null, name: "Before break", color: null },
    { anchor: "drop", occurrence: 1, offsetBars: 0, kind: "hotcue", hot: 2, loopBars: null, name: "Drop", color: null },
    { anchor: "outro", occurrence: 1, offsetBars: 0, kind: "loop", hot: null, loopBars: 4, name: "Outro loop", color: null },
  ],
});

export function CueTemplateDialog({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t, bump } = useApp();
  const [list, setList] = useState<CueTemplate[]>([]);
  const [edit, setEdit] = useState<CueTemplate | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<{ created: number; skipped: [string, string][] } | null>(null);

  const load = () =>
    api.listCueTemplates().then((l) => {
      setList(l);
      setEdit((cur) => cur ?? (l[0] ? structuredClone(l[0]) : blank()));
    }, (e) => setError(String(e)));
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setSlot = (i: number, patch: Partial<CueTemplateSlot>) =>
    setEdit((e) => e && { ...e, slots: e.slots.map((s, j) => (j === i ? { ...s, ...patch } : s)) });

  const save = async () => {
    if (!edit) return null;
    try {
      const saved = await api.saveCueTemplate(edit);
      setEdit(saved);
      setError(null);
      await load();
      return saved;
    } catch (e) {
      setError(String(e));
      return null;
    }
  };

  const apply = async () => {
    const saved = await save();
    if (!saved) return;
    setBusy(true);
    try {
      setReport(await api.applyCueTemplate(saved, ids));
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
        <h3>{t("cueTpl.title")}</h3>
        {error && <div className="query-error">{error}</div>}
        {report ? (
          <>
            <p className="ok">{t("cueTpl.done", { created: report.created, skipped: report.skipped.length })}</p>
            {report.skipped.length > 0 && (
              <ul className="small muted">
                {[...new Set(report.skipped.map(([, r]) => r))].slice(0, 12).map((r) => (
                  <li key={r}>
                    {r} ({report.skipped.filter(([, x]) => x === r).length})
                  </li>
                ))}
              </ul>
            )}
            <p className="hint">{t("cueTpl.undoHint")}</p>
            <div className="row end">
              <button className="btn" onClick={onClose}>
                {t("action.ok")}
              </button>
            </div>
          </>
        ) : (
          edit && (
            <>
              <div className="row wrap">
                <select
                  value={edit.id}
                  onChange={(e) => {
                    const found = list.find((x) => x.id === e.target.value);
                    setEdit(found ? structuredClone(found) : blank());
                  }}
                >
                  {list.map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.name}
                    </option>
                  ))}
                  <option value="">+ {t("cueTpl.new")}</option>
                </select>
                <input placeholder={t("cueTpl.name")} value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
                {edit.id && (
                  <button
                    className="btn ghost"
                    onClick={() =>
                      api.deleteCueTemplate(edit.id).then(() => {
                        setEdit(null);
                        load();
                      })
                    }
                  >
                    {t("action.delete")}
                  </button>
                )}
              </div>
              <table className="grid-table small">
                <thead>
                  <tr>
                    <th>{t("cueTpl.kind")}</th>
                    <th>{t("cueTpl.anchor")}</th>
                    <th>#</th>
                    <th>{t("cueTpl.offset")}</th>
                    <th>{t("cueTpl.label")}</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {edit.slots.map((s, i) => (
                    <tr key={i}>
                      <td>
                        <select
                          value={s.kind === "hotcue" ? `hot${s.hot ?? 0}` : s.kind}
                          onChange={(e) => {
                            const v = e.target.value;
                            if (v.startsWith("hot")) setSlot(i, { kind: "hotcue", hot: Number(v.slice(3)), loopBars: null });
                            else if (v === "loop") setSlot(i, { kind: "loop", hot: null, loopBars: s.loopBars ?? 4 });
                            else setSlot(i, { kind: "memory", hot: null, loopBars: null });
                          }}
                        >
                          {[...LETTERS].map((l, j) => (
                            <option key={l} value={`hot${j}`}>
                              {t("cueTpl.hot", { l })}
                            </option>
                          ))}
                          <option value="memory">{t("cueTpl.memory")}</option>
                          <option value="loop">{t("cueTpl.loop")}</option>
                        </select>
                        {s.kind === "loop" && (
                          <select value={s.loopBars ?? 4} onChange={(e) => setSlot(i, { loopBars: Number(e.target.value) })}>
                            {[1, 2, 4, 8, 16].map((b) => (
                              <option key={b} value={b}>
                                {t("cueTpl.bars", { n: b })}
                              </option>
                            ))}
                          </select>
                        )}
                      </td>
                      <td>
                        <select value={s.anchor} onChange={(e) => setSlot(i, { anchor: e.target.value as CueAnchor })}>
                          {ANCHORS.map((a) => (
                            <option key={a} value={a}>
                              {t(`cueTpl.a.${a}` as MsgKey)}
                            </option>
                          ))}
                        </select>
                      </td>
                      <td>
                        {(s.anchor === "breakdown" || s.anchor === "drop") && (
                          <input type="number" min={1} max={8} className="num narrow" value={s.occurrence} onChange={(e) => setSlot(i, { occurrence: Math.max(1, Number(e.target.value) || 1) })} />
                        )}
                      </td>
                      <td>
                        <input type="number" step={1} className="num narrow" value={s.offsetBars} onChange={(e) => setSlot(i, { offsetBars: Number(e.target.value) || 0 })} />
                      </td>
                      <td>
                        <input value={s.name ?? ""} onChange={(e) => setSlot(i, { name: e.target.value })} />
                      </td>
                      <td>
                        <button className="btn ghost small" onClick={() => setEdit({ ...edit, slots: edit.slots.filter((_, j) => j !== i) })}>
                          ×
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="row">
                <button
                  className="btn ghost small"
                  onClick={() => setEdit({ ...edit, slots: [...edit.slots, { anchor: "firstBeat", occurrence: 1, offsetBars: 0, kind: "memory", hot: null, loopBars: null, name: null, color: null }] })}
                >
                  + {t("cueTpl.addSlot")}
                </button>
              </div>
              <label className="toggle">
                <input type="checkbox" checked={edit.replaceHot} onChange={(e) => setEdit({ ...edit, replaceHot: e.target.checked })} />
                {t("cueTpl.replaceHot")}
              </label>
              <p className="hint">{t("cueTpl.hint")}</p>
              <div className="row end">
                <button className="btn ghost" onClick={onClose}>
                  {t("action.cancel")}
                </button>
                <button className="btn ghost" onClick={save}>
                  {t("cueTpl.save")}
                </button>
                <button className="btn" disabled={busy || ids.length === 0} onClick={apply}>
                  {busy ? t("common.working") : t("cueTpl.apply", { n: ids.length })}
                </button>
              </div>
            </>
          )
        )}
      </div>
    </div>
  );
}
