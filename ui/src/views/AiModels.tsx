// Every audio model Cradex can use, with what it is good and bad at
// (measured where possible), its size and licence, and download / verify /
// remove. Models still being measured are listed as such, without a download.
import { useEffect, useState } from "react";
import { api, onEvent, type ModelInfo, type StylesStatus } from "../api";
import { formatBytes } from "../format";
import type { MsgKey } from "../i18n";
import { GenreSourceDialog } from "../components/GenreSourceDialog";
import { useAction, useApp } from "../store";

/** Models under evaluation: shown with their facts, not downloadable yet. */
const EVALUATING = [
  { id: "omar", sizeMb: 2300, license: "CC BY-NC-SA 4.0 — Music Technology Group, UPF (OMAR-RQ)" },
  { id: "muq", sizeMb: 1300, license: "CC BY-NC 4.0 — Tencent AI Lab (MuQ)" },
  { id: "mulan", sizeMb: 2650, license: "CC BY-NC 4.0 — Tencent AI Lab (MuQ-MuLan)" },
];

const DOWNLOAD_EVENT: Record<string, string> = { effnet: "styles-download", heads: "styles-download", maest: "styles-download", clap: "clap-download" };

export function AiModels() {
  const { t, confirm, toast, bump, dataVersion } = useApp();
  const act = useAction();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [styles, setStyles] = useState<StylesStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [genreOpen, setGenreOpen] = useState(false);

  const refresh = () => {
    api.modelsCatalog().then(setModels, () => setModels([]));
    api.stylesStatus().then(setStyles, () => setStyles(null));
  };
  useEffect(refresh, [dataVersion]);

  const download = async (id: string) => {
    setBusy(id);
    setProgress(null);
    const un = await onEvent<{ done: number; total: number | null }>(DOWNLOAD_EVENT[id], setProgress);
    try {
      const run = id === "clap" ? api.clapDownload : id === "maest" ? api.stylesDownloadAccurate : api.stylesDownload;
      await act(run, t("models.downloaded", { name: t(`models.${id}.name` as MsgKey) }));
    } finally {
      un();
      setBusy(null);
      setProgress(null);
      refresh();
      bump();
    }
  };

  const verify = async (id: string) => {
    setBusy(id);
    const r = await act(() => api.modelsVerify(id));
    setBusy(null);
    refresh();
    if (!r) return;
    if (r.damaged.length) toast(t("models.damaged", { files: r.damaged.join(", ") }), "error");
    else if (r.missing.length) toast(t("models.missingFiles", { n: r.missing.length }), "error");
    else toast(t("models.verified", { n: r.ok }), "ok");
  };

  const remove = async (m: ModelInfo) => {
    const name = t(`models.${m.id}.name` as MsgKey);
    if (!(await confirm(t("models.removeQ", { name }), t("models.removeBody", { mb: m.sizeMb }), true))) return;
    await act(() => api.modelsRemove(m.id), t("models.removed", { name }));
    refresh();
    bump();
  };

  const installedBytes = models.reduce((s, m) => s + m.installedBytes, 0);
  const genreSource = styles?.source ?? "labels";

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.models")}</h1>
        <span className="muted">{t("models.subtitle", { size: formatBytes(installedBytes) })}</span>
      </header>

      <section className="panel">
        <h2>{t("models.genreEngine")}</h2>
        <p className="hint">{t("models.genreEngineHint")}</p>
        <div className="row">
          <span>
            {t("styles.settingLabel")}: <strong>{t(`styles.opt.${genreSource}` as MsgKey)}</strong>
            {styles?.accurate && <span className="badge">MAEST</span>}
          </span>
          <button className="btn small ghost" onClick={() => setGenreOpen(true)}>
            {t("styles.change")}
          </button>
        </div>
        {genreOpen && (
          <GenreSourceDialog
            onClose={() => {
              setGenreOpen(false);
              refresh();
            }}
          />
        )}
      </section>

      <div className="model-grid">
        {models.map((m) => (
          <article key={m.id} className={`model-card ${m.installed ? "installed" : ""}`}>
            <header>
              <h3>{t(`models.${m.id}.name` as MsgKey)}</h3>
              <span className={`badge ${m.installed ? "ok" : ""}`}>
                {m.bundled ? t("models.bundled") : m.installed ? t("models.installed") : t("models.notInstalled")}
              </span>
            </header>
            <p className="model-what">{t(`models.${m.id}.what` as MsgKey)}</p>
            <ProsCons id={m.id} />
            <dl className="model-facts">
              <dt>{t("models.size")}</dt>
              <dd>{m.installed && m.installedBytes ? formatBytes(m.installedBytes) : `${m.sizeMb} MB`}</dd>
              <dt>{t("models.speed")}</dt>
              <dd>{t(`models.${m.id}.speed` as MsgKey)}</dd>
              <dt>{t("models.license")}</dt>
              <dd>{m.license}</dd>
            </dl>
            {busy === m.id && progress && (
              <progress max={progress.total ?? undefined} value={progress.done} className="model-progress" />
            )}
            <div className="row">
              {!m.bundled && !m.installed && (
                <button className="btn small" disabled={busy !== null} onClick={() => download(m.id)}>
                  {busy === m.id ? t("common.working") : t("models.download", { mb: m.sizeMb })}
                </button>
              )}
              {!m.bundled && m.installed && (
                <>
                  <button className="btn small ghost" disabled={busy !== null} onClick={() => verify(m.id)}>
                    {busy === m.id ? t("common.working") : t("models.verify")}
                  </button>
                  <button className="btn small ghost danger" disabled={busy !== null} onClick={() => remove(m)}>
                    {t("models.remove")}
                  </button>
                </>
              )}
            </div>
          </article>
        ))}
        {EVALUATING.map((m) => (
          <article key={m.id} className="model-card evaluating">
            <header>
              <h3>{t(`models.${m.id}.name` as MsgKey)}</h3>
              <span className="badge">{t("models.evaluating")}</span>
            </header>
            <p className="model-what">{t(`models.${m.id}.what` as MsgKey)}</p>
            <ProsCons id={m.id} />
            <dl className="model-facts">
              <dt>{t("models.size")}</dt>
              <dd>~{m.sizeMb} MB</dd>
              <dt>{t("models.speed")}</dt>
              <dd>{t(`models.${m.id}.speed` as MsgKey)}</dd>
              <dt>{t("models.license")}</dt>
              <dd>{m.license}</dd>
            </dl>
            <p className="hint">{t("models.evaluatingHint")}</p>
          </article>
        ))}
      </div>
    </div>
  );
}

function ProsCons({ id }: { id: string }) {
  const { t } = useApp();
  const list = (k: string) =>
    t(`models.${id}.${k}` as MsgKey)
      .split("|")
      .map((s) => s.trim())
      .filter(Boolean);
  return (
    <div className="pros-cons">
      <ul className="pros">
        {list("pros").map((p) => (
          <li key={p}>{p}</li>
        ))}
      </ul>
      <ul className="cons">
        {list("cons").map((p) => (
          <li key={p}>{p}</li>
        ))}
      </ul>
    </div>
  );
}
