// How genres are suggested: the optional Discogs style model (downloaded
// from the MTG server, non-commercial licence), only the user's own labels,
// or both. Asked once, before the first analysis; changeable in Settings.
import { useEffect, useState } from "react";
import { api, onEvent, type StylesStatus } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

type Source = "both" | "discogs" | "labels";

let opener: ((done: () => void) => void) | null = null;

/** Ask for the genre source once (no-op when already chosen). */
export async function ensureGenreSource(): Promise<void> {
  const s = await api.getSettings().catch(() => ({}) as Record<string, string>);
  if (s.genreSource || !opener) return;
  const open = opener;
  await new Promise<void>((resolve) => open(resolve));
}

/** Mounted once in the app: shows the dialog when `ensureGenreSource` asks. */
export function GenreSourceHost() {
  const [done, setDone] = useState<(() => void) | null>(null);
  useEffect(() => {
    opener = (d) => setDone(() => d);
    return () => {
      opener = null;
    };
  }, []);
  if (!done) return null;
  return (
    <GenreSourceDialog
      onClose={() => {
        done();
        setDone(null);
      }}
    />
  );
}

export function GenreSourceDialog({ onClose }: { onClose: () => void }) {
  const { t, toast, bump } = useApp();
  const [st, setSt] = useState<StylesStatus | null>(null);
  const [choice, setChoice] = useState<Source>("both");
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.stylesStatus().then(
      (s) => {
        setSt(s);
        if (s.source === "discogs" || s.source === "labels" || s.source === "both") setChoice(s.source);
      },
      () => {},
    );
  }, []);

  const save = async (source: Source) => {
    setBusy(true);
    try {
      await api.setSetting("genreSource", source);
      if (source !== "labels" && !st?.downloaded) {
        const un = await onEvent<{ done: number; total: number | null }>("styles-download", setProgress);
        try {
          await api.stylesDownload();
        } finally {
          un();
        }
        toast(t("styles.downloaded"), "ok");
      }
      bump();
      onClose();
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const options: Source[] = ["both", "discogs", "labels"];
  return (
    <div className="modal-backdrop">
      <div className="modal wide" role="dialog" aria-modal>
        <h3>{t("styles.title")}</h3>
        <p className="muted">{t("styles.intro")}</p>
        <div className="stack">
          {options.map((o) => (
            <label key={o} className="toggle choice">
              <input type="radio" name="genre-source" checked={choice === o} onChange={() => setChoice(o)} disabled={busy} />
              <span>
                <strong>{t(`styles.opt.${o}` as MsgKey)}</strong>
                <br />
                <span className="hint">{t(`styles.opt.${o}Hint` as MsgKey)}</span>
              </span>
            </label>
          ))}
        </div>
        {choice !== "labels" && (
          <div className="panel warn-box">
            <p className="small">
              <strong>{t("styles.licenseTitle")}</strong> {st?.license}
            </p>
            <p className="hint">{t("styles.licenseHint", { mb: st?.sizeMb ?? 18 })}</p>
          </div>
        )}
        {progress && (
          <p className="small">
            {t("styles.downloading", {
              done: Math.round(progress.done / 1048576),
              total: progress.total ? Math.round(progress.total / 1048576) : "?",
            })}
          </p>
        )}
        <div className="row end">
          <button
            className="btn ghost"
            disabled={busy}
            onClick={async () => {
              // Deciding later keeps the current behaviour (own labels only).
              await api.setSetting("genreSource", st?.source ?? "labels").catch(() => {});
              onClose();
            }}
          >
            {t("styles.later")}
          </button>
          <button className="btn" disabled={busy} onClick={() => save(choice)}>
            {choice === "labels" || st?.downloaded ? t("action.ok") : t("styles.acceptDownload")}
          </button>
        </div>
      </div>
    </div>
  );
}
