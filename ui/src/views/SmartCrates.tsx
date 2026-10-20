import { useEffect, useState } from "react";
import { api, type Playlist } from "../api";
import { formatNumber } from "../format";
import { useAction, useApp } from "../store";

const TEMPLATES = [
  "decision:keep bpm:122-126",
  "genre:house -decision:pass rating:>=4",
  "key:8A+ bpm:120-128",
  "analyzed:yes lufs:>-9",
  "added:30d decision:none",
  "decision:maybe",
];

export function SmartCrates() {
  const { t, lang, dataVersion, bump, openLibrary, confirm } = useApp();
  const act = useAction();
  const [crates, setCrates] = useState<Playlist[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [name, setName] = useState("");
  const [rules, setRules] = useState("");
  const [preview, setPreview] = useState<{ n?: number; err?: string }>({});
  const [editing, setEditing] = useState<string | null>(null);

  useEffect(() => {
    api.listPlaylists().then((ps) => {
      const smart = ps.filter((p) => p.kind === "smart");
      setCrates(smart);
      smart.forEach((p) =>
        api.validateQuery(p.rules ?? "").then(
          (n) => setCounts((c) => ({ ...c, [p.id]: n })),
          () => {},
        ),
      );
    }, () => {});
  }, [dataVersion]);

  useEffect(() => {
    const h = setTimeout(() => {
      if (!rules.trim()) return setPreview({});
      api.validateQuery(rules).then(
        (n) => setPreview({ n }),
        (e) => setPreview({ err: String(e) }),
      );
    }, 200);
    return () => clearTimeout(h);
  }, [rules]);

  const save = async () => {
    if (!name.trim() || preview.err) return;
    if (editing) {
      // Rules are immutable per crate id for now: recreate with the same name.
      await act(() => api.deletePlaylist(editing));
    }
    const id = await act(() => api.createPlaylist(name.trim(), "smart", rules), t("crates.saved", { name }));
    if (id) {
      setName("");
      setRules("");
      setEditing(null);
      bump();
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.crates")}</h1>
        <span className="muted">{t("crates.subtitle")}</span>
      </header>
      <section className="panel">
        <h2>{editing ? t("crates.edit") : t("crates.new")}</h2>
        <div className="stack">
          <label className="field col">
            <span className="field-label">{t("crates.name")}</span>
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="field col">
            <span className="field-label">{t("crates.rules")}</span>
            <input value={rules} onChange={(e) => setRules(e.target.value)} spellCheck={false} className="mono" placeholder="decision:keep bpm:122-126 key:8A+" />
          </label>
          <div className={preview.err ? "query-error" : "muted"}>
            {preview.err ?? (preview.n != null ? t("crates.matches", { n: formatNumber(preview.n, lang) }) : t("crates.typeRules"))}
          </div>
          <div className="chips">
            {TEMPLATES.map((q) => (
              <button key={q} className="chip mono" onClick={() => setRules(q)}>
                {q}
              </button>
            ))}
          </div>
          <div className="row">
            <button className="btn" disabled={!name.trim() || !rules.trim() || !!preview.err} onClick={save}>
              {t("crates.save")}
            </button>
            {editing && (
              <button className="btn ghost" onClick={() => (setEditing(null), setName(""), setRules(""))}>
                {t("action.cancel")}
              </button>
            )}
          </div>
          <p className="hint">{t("crates.hint")}</p>
        </div>
      </section>
      <section className="panel">
        <h2>{t("crates.list")}</h2>
        {crates.length === 0 ? (
          <p className="muted">{t("crates.none")}</p>
        ) : (
          <table className="grid-table">
            <tbody>
              {crates.map((c) => (
                <tr key={c.id}>
                  <td>
                    <strong>⚡ {c.name}</strong>
                  </td>
                  <td className="mono small">{c.rules}</td>
                  <td className="num">{counts[c.id] != null ? formatNumber(counts[c.id], lang) : ""}</td>
                  <td className="row">
                    <button className="btn small" onClick={() => openLibrary({ playlistId: c.id })}>
                      {t("crates.open")}
                    </button>
                    <button className="btn small ghost" onClick={() => (setEditing(c.id), setName(c.name), setRules(c.rules ?? ""))}>
                      {t("crates.editBtn")}
                    </button>
                    <button
                      className="btn small ghost danger"
                      onClick={async () => {
                        if (await confirm(t("crates.deleteQ", { name: c.name }), undefined, true)) act(() => api.deletePlaylist(c.id)).then(bump);
                      }}
                    >
                      {t("action.delete")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}
