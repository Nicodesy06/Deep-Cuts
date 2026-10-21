import { useEffect, useState } from "react";
import { api, type BrokenPlaylist, type DuplicateGroup, type TrackRow, type TranscodeSuspect } from "../api";
import { MergeDialog } from "../components/MergeDialog";
import { open } from "@tauri-apps/plugin-dialog";
import { importFolders } from "../actions";
import type { MsgKey } from "../i18n";
import { formatBpm, formatDuration } from "../format";
import { SmartFixes } from "../components/SmartFixes";
import { useAction, useApp } from "../store";

export function LibraryHealth() {
  const { t, dataVersion, bump, openLibrary, play, toast } = useApp();
  const act = useAction();
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [suspects, setSuspects] = useState<TranscodeSuspect[] | null>(null);
  const [broken, setBroken] = useState<BrokenPlaylist[] | null>(null);
  const [merging, setMerging] = useState<{ tracks: TrackRow[]; kind: string } | null>(null);
  const [unused, setUnused] = useState<string[] | null>(null);
  const [unusedRoot, setUnusedRoot] = useState<string | null>(null);
  const [missing, setMissing] = useState<[number, number] | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    api.duplicateGroups().then(setGroups, () => setGroups([]));
    api.transcodeSuspects().then(setSuspects, () => setSuspects([]));
    api.brokenPlaylists().then(setBroken, () => setBroken([]));
  }, [dataVersion]);

  const check = async () => {
    setChecking(true);
    const r = await act(() => api.checkMissing());
    setChecking(false);
    if (r) {
      setMissing(r);
      bump();
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.health")}</h1>
        <span className="muted">{t("health.subtitle")}</span>
      </header>
      <section className="panel">
        <h2>{t("health.missingTitle")}</h2>
        <div className="row wrap">
          <button className="btn" onClick={check} disabled={checking}>
            {checking ? t("common.working") : t("health.checkMissing")}
          </button>
          {missing && <span className="muted">{t("health.missingResult", { checked: missing[0], missing: missing[1] })}</span>}
          <button className="btn ghost" onClick={() => openLibrary({ query: "missing:yes" })}>
            {t("health.showMissing")}
          </button>
        </div>
        <p className="hint">{t("health.relinkHint")}</p>
      </section>
      <section className="panel">
        <h2>{t("health.dupTitle")}</h2>
        <p className="hint">{t("health.dupHint")}</p>
        {groups == null ? (
          <p className="muted">{t("common.loading")}</p>
        ) : groups.length === 0 ? (
          <p className="muted">{t("health.noDups")}</p>
        ) : (
          groups.map((g) => (
            <div key={g.kind + g.key} className="dup-group">
              <div className="dup-head">
                <span className={`badge ${g.kind === "identical-file" ? "badge-warn" : ""}`}>{t(`health.${g.kind}` as never)}</span>
                <span className="muted small">{g.tracks.length} ×</span>
                <button className="btn small" onClick={() => setMerging({ tracks: g.tracks, kind: g.kind })}>
                  {t("merge.button")}
                </button>
              </div>
              <table className="grid-table">
                <tbody>
                  {g.tracks.map((tr) => (
                    <tr key={tr.id}>
                      <td>
                        <button className="icon-btn small" onClick={() => play(tr, tr.previewMs ?? 0)} aria-label={t("player.play")}>
                          ▶
                        </button>
                      </td>
                      <td>{tr.artist}</td>
                      <td>
                        {tr.title}
                        {tr.version && <span className="muted"> ({tr.version})</span>}
                      </td>
                      <td className="num">{formatBpm(tr.bpm)}</td>
                      <td className="num">{formatDuration(tr.durationMs)}</td>
                      <td className="muted small">{tr.format?.toUpperCase()} {tr.bitrate ? `${tr.bitrate}k` : ""}</td>
                      <td className="muted small path">{tr.path}</td>
                      <td>{tr.decision && <span className={`dec-dot dec-${tr.decision}`} />}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))
        )}
      </section>
      <section className="panel">
        <h2>{t("health.transTitle")}</h2>
        <p className="hint">{t("health.transHint")}</p>
        {suspects == null ? (
          <p className="muted">{t("common.loading")}</p>
        ) : suspects.length === 0 ? (
          <p className="muted">{t("health.noTrans")}</p>
        ) : (
          <>
            <table className="grid-table trans-table">
              <tbody>
                {suspects.map((s) => (
                  <tr key={s.track.id}>
                    <td>
                      <button className="icon-btn small" onClick={() => play(s.track, s.track.previewMs ?? 0)} aria-label={t("player.play")}>
                        ▶
                      </button>
                    </td>
                    <td>{s.track.artist}</td>
                    <td>{s.track.title}</td>
                    <td className="muted small">
                      {s.track.format?.toUpperCase()} {s.track.bitrate ? `${s.track.bitrate}k` : ""}
                    </td>
                    <td className="num">{s.cutoffHz != null ? `${(s.cutoffHz / 1000).toFixed(1)} kHz` : ""}</td>
                    <td>
                      <span className="badge badge-warn">{t(`health.trans.${s.reason}` as MsgKey)}</span>
                    </td>
                    <td className="muted small path">{s.track.path}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="row">
              <button className="btn ghost" onClick={() => openLibrary({ query: "transcode:yes" })}>
                {t("health.showTrans")}
              </button>
            </div>
          </>
        )}
      </section>
      <section className="panel">
        <h2>{t("health.brokenTitle")}</h2>
        <p className="hint">{t("health.brokenHint")}</p>
        {broken == null ? (
          <p className="muted">{t("common.loading")}</p>
        ) : broken.length === 0 ? (
          <p className="muted">{t("health.noBroken")}</p>
        ) : (
          <table className="grid-table">
            <tbody>
              {broken.map((b) => (
                <tr key={b.id}>
                  <td>
                    <button className="link-btn" onClick={() => openLibrary({ query: "missing:yes", playlistId: b.id })}>
                      {b.name}
                    </button>
                  </td>
                  <td className="muted small">{t("health.brokenCount", { missing: b.missing, total: b.total })}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      <SmartFixes />
      <section className="panel">
        <h2>{t("health.unusedTitle")}</h2>
        <p className="hint">{t("health.unusedHint")}</p>
        <div className="row">
          <button
            className="btn"
            onClick={async () => {
              const r = await open({ directory: true, multiple: false, title: t("health.unusedChoose") });
              if (typeof r !== "string") return;
              setUnused(null);
              setUnusedRoot(r);
              const list = await act(() => api.unusedFiles(r));
              setUnused(list ?? []);
            }}
          >
            {t("health.unusedScan")}
          </button>
          {unusedRoot && unused == null && <span className="muted">{t("common.working")}</span>}
          {unused != null && <span className="muted">{t("health.unusedResult", { n: unused.length })}</span>}
          {unused != null && unused.length > 0 && (
            <button className="btn" onClick={() => importFolders(t, toast, bump, unused).then(() => setUnused([]))}>
              {t("health.unusedImport", { n: unused.length })}
            </button>
          )}
        </div>
        {unused != null && unused.length > 0 && (
          <ul className="small path unused-list">
            {unused.slice(0, 300).map((f) => (
              <li key={f}>{f}</li>
            ))}
          </ul>
        )}
      </section>
      {merging && <MergeDialog tracks={merging.tracks} kind={merging.kind} onClose={() => setMerging(null)} />}
    </div>
  );
}
