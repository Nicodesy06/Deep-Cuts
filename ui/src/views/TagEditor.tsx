import { useEffect, useMemo, useState } from "react";
import { api, TAG_CATEGORIES, type Tag } from "../api";
import { quoteQueryValue } from "../format";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

/** Tags ordered as a tree (parent before children) with their depth. */
export function tagTree(tags: Tag[]): { tag: Tag; depth: number }[] {
  const byParent = new Map<string | null, Tag[]>();
  const ids = new Set(tags.map((t) => t.id));
  for (const t of tags) {
    const p = t.parentId && ids.has(t.parentId) ? t.parentId : null;
    byParent.set(p, [...(byParent.get(p) ?? []), t]);
  }
  const out: { tag: Tag; depth: number }[] = [];
  const walk = (parent: string | null, depth: number, seen: Set<string>) => {
    for (const t of (byParent.get(parent) ?? []).sort((a, b) => a.name.localeCompare(b.name))) {
      if (seen.has(t.id)) continue;
      seen.add(t.id);
      out.push({ tag: t, depth });
      walk(t.id, depth + 1, seen);
    }
  };
  walk(null, 0, new Set());
  return out;
}

export function TagEditor() {
  const { t, dataVersion, bump, openLibrary, confirm, prompt } = useApp();
  const act = useAction();
  const [tags, setTags] = useState<Tag[]>([]);
  const [name, setName] = useState("");
  const [category, setCategory] = useState<string>("custom");
  const [parent, setParent] = useState<string>("");
  const [selected, setSelected] = useState<string | null>(null);
  const [synonym, setSynonym] = useState("");

  useEffect(() => {
    api.listTags().then(setTags, () => {});
  }, [dataVersion]);

  const tree = useMemo(() => tagTree(tags), [tags]);
  const sel = tags.find((x) => x.id === selected) ?? null;
  const byCategory = useMemo(() => {
    const m = new Map<string, { tag: Tag; depth: number }[]>();
    for (const n of tree) m.set(n.tag.category, [...(m.get(n.tag.category) ?? []), n]);
    return m;
  }, [tree]);

  const create = async () => {
    if (!name.trim()) return;
    const id = await act(() => api.createTag(name, category, parent || null), t("tags.created", { name }));
    if (id) {
      setName("");
      setSelected(id);
      bump();
    }
  };

  const slots = Array.from({ length: 9 }, (_, i) => i + 1);

  return (
    <div className="page">
      <header className="page-head">
        <h1>{t("nav.tags")}</h1>
        <span className="muted">{t("tags.subtitle")}</span>
      </header>
      <div className="cols-2">
        <section className="panel">
          <h2>{t("tags.new")}</h2>
          <div className="row wrap">
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("tags.name")} onKeyDown={(e) => e.key === "Enter" && create()} />
            <select value={category} onChange={(e) => setCategory(e.target.value)} aria-label={t("tags.category")}>
              {TAG_CATEGORIES.map((c) => (
                <option key={c} value={c}>
                  {t(`tags.cat.${c}` as MsgKey)}
                </option>
              ))}
            </select>
            <select value={parent} onChange={(e) => setParent(e.target.value)} aria-label={t("tags.parent")}>
              <option value="">{t("tags.noParent")}</option>
              {tree.map(({ tag, depth }) => (
                <option key={tag.id} value={tag.id}>
                  {"  ".repeat(depth)}
                  {tag.name}
                </option>
              ))}
            </select>
            <button className="btn" disabled={!name.trim()} onClick={create}>
              {t("tags.create")}
            </button>
          </div>
          <p className="hint">{t("tags.hint")}</p>

          <h2 className="mt">{t("tags.quick")}</h2>
          <div className="quick-slots">
            {slots.map((s) => {
              const tag = tags.find((x) => x.quickSlot === s);
              return (
                <label key={s} className="quick-slot">
                  <span className="kbd">⇧{s}</span>
                  <select value={tag?.id ?? ""} onChange={(e) => act(() => api.setQuickTag(s, e.target.value || null)).then(bump)}>
                    <option value="">—</option>
                    {tree.map(({ tag: x, depth }) => (
                      <option key={x.id} value={x.id}>
                        {"  ".repeat(depth)}
                        {x.name}
                      </option>
                    ))}
                  </select>
                </label>
              );
            })}
          </div>
          <p className="hint">{t("tags.quickHint")}</p>
        </section>

        <section className="panel">
          <h2>{sel ? sel.name : t("tags.details")}</h2>
          {!sel ? (
            <p className="muted">{t("tags.pick")}</p>
          ) : (
            <div className="stack">
              <div className="row wrap">
                <button className="btn small" onClick={() => openLibrary({ query: `tag:${quoteQueryValue(sel.name)}` })}>
                  {t("tags.showTracks", { n: sel.trackCount })}
                </button>
                <button
                  className="btn small ghost"
                  onClick={async () => {
                    const n = await prompt(t("tags.rename"), sel.name);
                    if (n) act(() => api.updateTag(sel.id, { name: n })).then(bump);
                  }}
                >
                  {t("tags.rename")}
                </button>
                <button
                  className="btn small ghost danger"
                  onClick={async () => {
                    if (await confirm(t("tags.deleteQ", { name: sel.name, n: sel.trackCount }), undefined, true)) {
                      await act(() => api.deleteTag(sel.id));
                      setSelected(null);
                      bump();
                    }
                  }}
                >
                  {t("action.delete")}
                </button>
              </div>
              <label className="field col">
                <span className="field-label">{t("tags.category")}</span>
                <select value={sel.category} onChange={(e) => act(() => api.updateTag(sel.id, { category: e.target.value })).then(bump)}>
                  {TAG_CATEGORIES.map((c) => (
                    <option key={c} value={c}>
                      {t(`tags.cat.${c}` as MsgKey)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field col">
                <span className="field-label">{t("tags.parent")}</span>
                <select
                  value={sel.parentId ?? ""}
                  onChange={(e) => act(() => api.updateTag(sel.id, { parentId: e.target.value || null })).then(bump)}
                >
                  <option value="">{t("tags.noParent")}</option>
                  {tree
                    .filter(({ tag }) => tag.id !== sel.id)
                    .map(({ tag, depth }) => (
                      <option key={tag.id} value={tag.id}>
                        {"  ".repeat(depth)}
                        {tag.name}
                      </option>
                    ))}
                </select>
              </label>
              <div className="field col">
                <span className="field-label">{t("tags.synonyms")}</span>
                <div className="chips">
                  {sel.synonyms.map((s) => (
                    <span key={s} className="chip">
                      {s}
                      <button className="chip-x" onClick={() => act(() => api.removeTagSynonym(s)).then(bump)} aria-label={t("action.delete")}>
                        ×
                      </button>
                    </span>
                  ))}
                </div>
                <div className="row">
                  <input value={synonym} onChange={(e) => setSynonym(e.target.value)} placeholder={t("tags.addSynonym")} />
                  <button
                    className="btn small"
                    disabled={!synonym.trim()}
                    onClick={() => act(() => api.addTagSynonym(sel.id, synonym)).then(() => (setSynonym(""), bump()))}
                  >
                    +
                  </button>
                </div>
              </div>
            </div>
          )}
        </section>
      </div>

      <section className="panel">
        <h2>{t("tags.all", { n: tags.length })}</h2>
        {tags.length === 0 && <p className="muted">{t("tags.none")}</p>}
        <div className="tag-columns">
          {TAG_CATEGORIES.filter((c) => byCategory.has(c)).map((c) => (
            <div key={c} className="tag-col">
              <div className="section-title">{t(`tags.cat.${c}` as MsgKey)}</div>
              {byCategory.get(c)!.map(({ tag, depth }) => (
                <button
                  key={tag.id}
                  className={`tag-row ${selected === tag.id ? "active" : ""}`}
                  style={{ paddingLeft: 8 + depth * 14 }}
                  onClick={() => setSelected(tag.id)}
                >
                  <span>{tag.name}</span>
                  <span className="muted small num">
                    {tag.quickSlot ? <span className="kbd">⇧{tag.quickSlot}</span> : null} {tag.trackCount}
                  </span>
                </button>
              ))}
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
