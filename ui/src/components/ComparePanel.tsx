// Compare Cradex' BPM/key with another source on the user's own library
// (imported Rekordbox/Traktor/Serato/VirtualDJ values or the files' tags).
import { useEffect, useState } from "react";
import { api, type CompareReport, type CompareSource } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

const pct = (n: number, d: number) => (d ? `${((n / d) * 100).toFixed(1)} %` : "—");

export function ComparePanel() {
  const { t, dataVersion, openLibrary } = useApp();
  const [sources, setSources] = useState<CompareSource[]>([]);
  const [source, setSource] = useState("");
  const [rep, setRep] = useState<CompareReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.compareSources().then((s) => {
      setSources(s);
      setSource((cur) => cur || (s.find((x) => x.source === "import:rekordbox") ?? s[0])?.source || "");
    }, () => {});
  }, [dataVersion]);

  useEffect(() => {
    if (!source) return;
    setRep(null);
    api.compareAnalysis(source).then(setRep, (e) => setError(String(e)));
  }, [source, dataVersion]);

  const label = (s: string) => (s === "file" ? t("compare.fileTags") : s.replace(/^import:/, ""));

  return (
    <section className="panel">
      <h2>{t("compare.title")}</h2>
      <p className="hint">{t("compare.hint")}</p>
      {sources.length === 0 ? (
        <p className="muted">{t("compare.none")}</p>
      ) : (
        <>
          <label className="field-inline">
            {t("compare.source")}
            <select value={source} onChange={(e) => setSource(e.target.value)}>
              {sources.map((s) => (
                <option key={s.source} value={s.source}>
                  {label(s.source)} ({s.bpm} BPM · {s.key} {t("compare.keys")})
                </option>
              ))}
            </select>
          </label>
          {error && <div className="query-error">{error}</div>}
          {rep && (
            <>
              <div className="cols-2">
                <table className="grid-table small">
                  <tbody>
                    <tr><th colSpan={2}>BPM ({rep.bpmCompared})</th></tr>
                    <tr><td>{t("compare.bpmExact")}</td><td className="num">{pct(rep.bpmExact, rep.bpmCompared)}</td></tr>
                    <tr><td>{t("compare.bpmClose")}</td><td className="num">{pct(rep.bpmClose, rep.bpmCompared)}</td></tr>
                    <tr><td>{t("compare.bpmOctave")}</td><td className="num">{pct(rep.bpmOctave, rep.bpmCompared)}</td></tr>
                    <tr><td>{t("compare.bpmOther")}</td><td className="num">{pct(rep.bpmOther, rep.bpmCompared)}</td></tr>
                  </tbody>
                </table>
                <table className="grid-table small">
                  <tbody>
                    <tr><th colSpan={2}>{t("compare.key")} ({rep.keyCompared})</th></tr>
                    <tr><td>{t("compare.keySame")}</td><td className="num">{pct(rep.keySame, rep.keyCompared)}</td></tr>
                    <tr><td>{t("compare.keyNear")}</td><td className="num">{pct(rep.keyFifth + rep.keyRelative + rep.keyParallel, rep.keyCompared)}</td></tr>
                    <tr><td>{t("compare.keyOther")}</td><td className="num">{pct(rep.keyOther, rep.keyCompared)}</td></tr>
                  </tbody>
                </table>
              </div>
              {rep.bpmOctave > 0 && <p className="hint">{t("compare.octaveHint")}</p>}
              {rep.differences.length > 0 && (
                <div className="write-plan">
                  <table className="grid-table small">
                    <thead>
                      <tr>
                        <th>{t("col.artist")}</th>
                        <th>{t("col.title")}</th>
                        <th>Cradex</th>
                        <th>{label(rep.source)}</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {rep.differences.map((d) => (
                        <tr key={d.track.id + d.kind} className="clickable" onClick={() => openLibrary({ query: `"${(d.track.title ?? "").replace(/"/g, "")}"`, playlistId: null })}>
                          <td>{d.track.artist}</td>
                          <td>{d.track.title}</td>
                          <td className="num">{d.ours}</td>
                          <td className="num">{d.theirs}</td>
                          <td className="muted">{t(`compare.kind.${d.kind}` as MsgKey)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </>
      )}
    </section>
  );
}
