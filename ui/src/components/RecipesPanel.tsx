// Recipes: a saved search + actions, run on demand or on newly imported tracks.
import { useEffect, useState } from "react";
import { api, type Playlist, type Recipe, type RecipeAction } from "../api";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

const FIELDS = ["genre", "subgenre", "comment", "label", "artist", "title", "version", "remixer", "album", "year", "rating"];
const EMPTY: Recipe = { id: "", name: "", query: "", actions: [{ type: "addTag", tag: "" }], onImport: false, lastRun: null };

function describe(a: RecipeAction, t: (k: MsgKey, v?: Record<string, string | number>) => string, playlists: Playlist[]): string {
  switch (a.type) {
    case "setField":
      return t("recipe.d.setField", { field: t(`col.${a.field}` as MsgKey), value: a.value ?? "∅" });
    case "addTag":
      return t("recipe.d.addTag", { tag: a.tag });
    case "removeTag":
      return t("recipe.d.removeTag", { tag: a.tag });
    case "decision":
      return t("recipe.d.decision", { value: a.value ?? "∅" });
    case "addToPlaylist":
      return t("recipe.d.addToPlaylist", { name: playlists.find((p) => p.id === a.playlist)?.name ?? "?" });
    case "analyze":
      return t("recipe.d.analyze");
  }
}

function ActionEditor({ a, playlists, onChange, onRemove }: { a: RecipeAction; playlists: Playlist[]; onChange: (a: RecipeAction) => void; onRemove: () => void }) {
  const { t } = useApp();
  const setType = (type: RecipeAction["type"]) => {
    const init: Record<RecipeAction["type"], RecipeAction> = {
      setField: { type: "setField", field: "genre", value: "" },
      addTag: { type: "addTag", tag: "" },
      removeTag: { type: "removeTag", tag: "" },
      decision: { type: "decision", value: "maybe" },
      addToPlaylist: { type: "addToPlaylist", playlist: playlists[0]?.id ?? "" },
      analyze: { type: "analyze" },
    };
    onChange(init[type]);
  };
  return (
    <div className="row wrap recipe-action">
      <select value={a.type} onChange={(e) => setType(e.target.value as RecipeAction["type"])}>
        {(["setField", "addTag", "removeTag", "decision", "addToPlaylist", "analyze"] as const).map((x) => (
          <option key={x} value={x}>
            {t(`recipe.type.${x}` as MsgKey)}
          </option>
        ))}
      </select>
      {a.type === "setField" && (
        <>
          <select value={a.field} onChange={(e) => onChange({ ...a, field: e.target.value })}>
            {FIELDS.map((f) => (
              <option key={f} value={f}>
                {t(`col.${f}` as MsgKey)}
              </option>
            ))}
          </select>
          <input value={a.value ?? ""} onChange={(e) => onChange({ ...a, value: e.target.value })} placeholder={t("recipe.emptyClears")} />
        </>
      )}
      {(a.type === "addTag" || a.type === "removeTag") && <input value={a.tag} onChange={(e) => onChange({ ...a, tag: e.target.value })} placeholder={t("recipe.tagName")} />}
      {a.type === "decision" && (
        <select value={a.value ?? ""} onChange={(e) => onChange({ ...a, value: e.target.value || null })}>
          <option value="keep">{t("decision.keep" as MsgKey)}</option>
          <option value="maybe">{t("decision.maybe" as MsgKey)}</option>
          <option value="pass">{t("decision.pass" as MsgKey)}</option>
          <option value="">{t("decision.clear")}</option>
        </select>
      )}
      {a.type === "addToPlaylist" && (
        <select value={a.playlist} onChange={(e) => onChange({ ...a, playlist: e.target.value })}>
          {playlists.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      )}
      <button className="icon-btn small" onClick={onRemove} aria-label={t("action.delete")}>
        ×
      </button>
    </div>
  );
}

export function RecipesPanel() {
  const { t, bump, dataVersion, confirm, toast } = useApp();
  const act = useAction();
  const [list, setList] = useState<Recipe[]>([]);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [edit, setEdit] = useState<Recipe | null>(null);
  const [count, setCount] = useState<number | null>(null);

  const refresh = () => api.listRecipes().then(setList, () => {});
  useEffect(() => {
    refresh();
    api.listPlaylists().then((ps) => setPlaylists(ps.filter((p) => p.kind === "manual" || p.kind === "set")), () => {});
  }, [dataVersion]);
  useEffect(() => {
    if (!edit) return setCount(null);
    const h = setTimeout(() => api.recipeCount(edit).then(setCount, () => setCount(null)), 300);
    return () => clearTimeout(h);
  }, [edit]);

  return (
    <section className="panel">
      <div className="row space">
        <h2>{t("recipe.title")}</h2>
        {!edit && (
          <button className="btn small" onClick={() => setEdit({ ...EMPTY })}>
            + {t("recipe.new")}
          </button>
        )}
      </div>
      <p className="hint">{t("recipe.hint")}</p>
      {edit && (
        <div className="recipe-edit">
          <div className="fr-grid">
            <label className="field">
              <span>{t("recipe.name")}</span>
              <input value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
            </label>
            <label className="field">
              <span>{t("recipe.query")}</span>
              <input className="mono" value={edit.query} onChange={(e) => setEdit({ ...edit, query: e.target.value })} placeholder={t("recipe.queryAll")} spellCheck={false} />
            </label>
          </div>
          <p className="muted small">{count != null ? t("recipe.matches", { n: count }) : "\u00a0"}</p>
          {edit.actions.map((a, i) => (
            <ActionEditor
              key={i}
              a={a}
              playlists={playlists}
              onChange={(n) => setEdit({ ...edit, actions: edit.actions.map((x, j) => (j === i ? n : x)) })}
              onRemove={() => setEdit({ ...edit, actions: edit.actions.filter((_, j) => j !== i) })}
            />
          ))}
          <div className="row wrap">
            <button className="btn small ghost" onClick={() => setEdit({ ...edit, actions: [...edit.actions, { type: "addTag", tag: "" }] })}>
              + {t("recipe.addAction")}
            </button>
            <label className="toggle">
              <input type="checkbox" checked={edit.onImport} onChange={(e) => setEdit({ ...edit, onImport: e.target.checked })} />
              {t("recipe.onImport")}
            </label>
          </div>
          <div className="row end">
            <button className="btn ghost" onClick={() => setEdit(null)}>
              {t("action.cancel")}
            </button>
            <button
              className="btn"
              onClick={async () => {
                const id = await act(() => api.saveRecipe(edit), t("recipe.saved"));
                if (id) {
                  setEdit(null);
                  refresh();
                }
              }}
            >
              {t("recipe.save")}
            </button>
          </div>
        </div>
      )}
      {list.length === 0 && !edit && <p className="muted">{t("recipe.none")}</p>}
      <ul className="recipe-list">
        {list.map((r) => (
          <li key={r.id}>
            <div className="row space">
              <strong>{r.name}</strong>
              <div className="row">
                {r.onImport && <span className="badge">{t("recipe.onImportBadge")}</span>}
                <button
                  className="btn small"
                  onClick={() =>
                    act(() => api.runRecipe(r.id)).then((rep) => {
                      if (rep) {
                        bump();
                        refresh();
                        toast(t("recipe.ran", { tracks: rep.tracks, changes: rep.changes }), "ok");
                      }
                    })
                  }
                >
                  {t("recipe.run")}
                </button>
                <button className="btn small ghost" onClick={() => setEdit(r)}>
                  {t("recipe.edit")}
                </button>
                <button
                  className="icon-btn small"
                  onClick={async () => {
                    if (await confirm(t("recipe.deleteQ", { name: r.name }), undefined, true)) act(() => api.deleteRecipe(r.id)).then(refresh);
                  }}
                  aria-label={t("action.delete")}
                >
                  ×
                </button>
              </div>
            </div>
            <div className="muted small">
              <code>{r.query || t("recipe.queryAll")}</code> → {r.actions.map((a) => describe(a, t, playlists)).join(" · ")}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
