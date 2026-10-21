// What goes into the exported Comment and Genre fields, and whether set
// chapters are exported as their own crates.
import { useEffect, useState } from "react";
import { api } from "../api";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

const PARTS = [
  "comment",
  "tags",
  "energy",
  "key",
  "notes",
  "decision",
  "rating",
] as const;

interface Mapping {
  comment: string[];
  genreWithSubgenre: boolean;
}

export function ExportMappingPanel() {
  const { t } = useApp();
  const act = useAction();
  const [m, setM] = useState<Mapping>({
    comment: ["comment"],
    genreWithSubgenre: false,
  });
  const [chapters, setChapters] = useState(true);

  useEffect(() => {
    api.getSettings().then(
      (s) => {
        try {
          if (s.exportMapping) setM(JSON.parse(s.exportMapping));
        } catch {
          /* keep defaults */
        }
        setChapters(s.exportChapters !== "false");
      },
      () => {},
    );
  }, []);

  const saveMapping = (next: Mapping) => {
    setM(next);
    act(() => api.setSetting("exportMapping", JSON.stringify(next)));
  };
  const example = m.comment
    .map((p) => t(`mapping.example.${p}` as MsgKey))
    .filter(Boolean)
    .join(" · ");

  return (
    <section className="panel">
      <h2>{t("mapping.title")}</h2>
      <p className="hint">{t("mapping.hint")}</p>
      <div className="row wrap">
        {PARTS.map((p) => (
          <label key={p} className="toggle">
            <input
              type="checkbox"
              checked={m.comment.includes(p)}
              onChange={(e) =>
                saveMapping({
                  ...m,
                  comment: e.target.checked
                    ? PARTS.filter((x) => x === p || m.comment.includes(x))
                    : m.comment.filter((x) => x !== p),
                })
              }
            />
            {t(`mapping.part.${p}` as MsgKey)}
          </label>
        ))}
      </div>
      <p className="muted small">
        {t("mapping.preview")}: <span className="mono">{example || "∅"}</span>
      </p>
      <div className="stack">
        <label className="toggle">
          <input
            type="checkbox"
            checked={m.genreWithSubgenre}
            onChange={(e) =>
              saveMapping({ ...m, genreWithSubgenre: e.target.checked })
            }
          />
          {t("mapping.genreSub")}
        </label>
        <label className="toggle">
          <input
            type="checkbox"
            checked={chapters}
            onChange={(e) => {
              setChapters(e.target.checked);
              act(() =>
                api.setSetting(
                  "exportChapters",
                  e.target.checked ? "true" : "false",
                ),
              );
            }}
          />
          {t("mapping.chapters")}
        </label>
      </div>
      <p className="hint">{t("mapping.smartHint")}</p>
    </section>
  );
}
