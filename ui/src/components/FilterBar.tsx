// Filters as removable "bubbles" next to the search box. Each bubble is a
// small form (range, list, yes/no) that compiles to a fragment of the query
// language, so nobody has to learn the syntax to filter the library.
import { useEffect, useRef, useState } from "react";
import { api, type Tag } from "../api";
import type { MsgKey } from "../i18n";
import { useApp } from "../store";

export type FilterKind =
  | "bpm"
  | "key"
  | "genre"
  | "artist"
  | "label"
  | "tag"
  | "year"
  | "rating"
  | "energy"
  | "duration"
  | "decision"
  | "added"
  | "status";

export interface Filter {
  kind: FilterKind;
  /** Range start, or the single value (text, key, decision…). */
  a: string;
  /** Range end, or a mode (key: "", "+", "++"). */
  b: string;
  /** Exclude matching tracks instead. */
  not: boolean;
}

export const FILTER_KINDS: FilterKind[] = ["bpm", "key", "genre", "artist", "label", "tag", "year", "rating", "energy", "duration", "decision", "added", "status"];

const RANGE: Partial<Record<FilterKind, { field: string; min: number; max: number; step: number }>> = {
  bpm: { field: "bpm", min: 40, max: 250, step: 1 },
  year: { field: "year", min: 1900, max: 2100, step: 1 },
  energy: { field: "energy", min: 1, max: 10, step: 1 },
  duration: { field: "duration", min: 0, max: 60, step: 0.5 },
};

const KEYS = Array.from({ length: 12 }, (_, i) => [`${i + 1}A`, `${i + 1}B`]).flat();
const STATUS = ["analyzed:no", "analyzed:yes", "missing:yes", "artwork:no", "fav:yes", "genre:none", "key:none", "bpm:none"];

/** Text values with spaces must be quoted, which means an exact match. */
const textValue = (v: string) => (/[\s"():]/.test(v) ? `"${v.replace(/"/g, "")}"` : v);

function rangeQuery(field: string, a: string, b: string, scale = 1) {
  const n = (s: string) => String(Math.round(Number(s.replace(",", ".")) * scale * 100) / 100);
  const has = (s: string) => s.trim() !== "" && !Number.isNaN(Number(s.replace(",", ".")));
  if (has(a) && has(b)) return `${field}:${n(a)}-${n(b)}`;
  if (has(a)) return `${field}:>=${n(a)}`;
  if (has(b)) return `${field}:<=${n(b)}`;
  return "";
}

export function filterQuery(f: Filter): string {
  let q = "";
  switch (f.kind) {
    case "bpm":
    case "year":
    case "energy":
      q = rangeQuery(f.kind, f.a, f.b);
      break;
    case "duration":
      q = rangeQuery("duration", f.a, f.b, 60); // minutes in the form, seconds in the query
      break;
    case "key":
      q = f.a ? `key:${f.a}${f.b}` : "";
      break;
    case "genre":
    case "artist":
    case "label":
      q = f.a.trim() ? `${f.kind}:${textValue(f.a.trim())}` : "";
      break;
    case "tag":
      q = f.a ? `tag:${textValue(f.a)}` : "";
      break;
    case "rating":
      q = f.a ? `rating:>=${f.a}` : "";
      break;
    case "decision":
      q = f.a ? `decision:${f.a}` : "";
      break;
    case "added":
      q = f.a ? `added:${f.a}d` : "";
      break;
    case "status":
      q = f.a;
      break;
  }
  return q && f.not ? `-${q}` : q;
}

export function filtersQuery(text: string, filters: Filter[]) {
  return [text.trim(), ...filters.map(filterQuery)].filter(Boolean).join(" ");
}

function blank(kind: FilterKind): Filter {
  const defaults: Partial<Record<FilterKind, [string, string]>> = {
    key: ["8A", "+"],
    rating: ["4", ""],
    decision: ["keep", ""],
    added: ["7", ""],
    status: ["analyzed:no", ""],
  };
  const [a, b] = defaults[kind] ?? ["", ""];
  return { kind, a, b, not: false };
}

export function FilterBar({ filters, onChange }: { filters: Filter[]; onChange: (f: Filter[]) => void }) {
  const { t } = useApp();
  const [menu, setMenu] = useState(false);
  // Index being edited; filters.length = a new one.
  const [edit, setEdit] = useState<{ index: number; draft: Filter } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu && !edit) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setMenu(false);
        setEdit(null);
      }
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMenu(false);
        setEdit(null);
      }
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [menu, edit]);

  const save = () => {
    if (!edit) return;
    if (!filterQuery(edit.draft)) return setEdit(null);
    const next = [...filters];
    next.splice(edit.index, edit.index < filters.length ? 1 : 0, edit.draft);
    onChange(next);
    setEdit(null);
  };

  return (
    <div className="filter-bar" ref={ref}>
      <button
        className={`btn ghost small ${menu ? "active" : ""}`}
        aria-haspopup="menu"
        aria-expanded={menu}
        onClick={() => {
          setEdit(null);
          setMenu((v) => !v);
        }}
      >
        ＋ {t("filter.add")}
      </button>
      {filters.map((f, i) => (
        <span key={i} className={`filter-chip ${f.not ? "not" : ""}`}>
          <button className="filter-chip-label" onClick={() => setEdit({ index: i, draft: { ...f } })} title={filterQuery(f)}>
            {chipLabel(f, t)}
          </button>
          <button className="filter-chip-x" aria-label={t("action.clear")} onClick={() => onChange(filters.filter((_, j) => j !== i))}>
            ×
          </button>
        </span>
      ))}
      {filters.length > 1 && (
        <button className="link-btn small" onClick={() => onChange([])}>
          {t("filter.clearAll")}
        </button>
      )}
      {menu && (
        <div className="filter-menu" role="menu">
          {FILTER_KINDS.map((k) => (
            <button
              key={k}
              role="menuitem"
              onClick={() => {
                setMenu(false);
                setEdit({ index: filters.length, draft: blank(k) });
              }}
            >
              {t(`filter.kind.${k}` as MsgKey)}
            </button>
          ))}
        </div>
      )}
      {edit && <FilterEditor draft={edit.draft} onDraft={(draft) => setEdit({ ...edit, draft })} onSave={save} onCancel={() => setEdit(null)} />}
    </div>
  );
}

function chipLabel(f: Filter, t: ReturnType<typeof useApp>["t"]) {
  const name = t(`filter.kind.${f.kind}` as MsgKey);
  const range = (unit = "") => {
    if (f.a && f.b) return `${f.a}–${f.b}${unit}`;
    if (f.a) return `≥ ${f.a}${unit}`;
    return `≤ ${f.b}${unit}`;
  };
  let v: string;
  switch (f.kind) {
    case "bpm":
    case "year":
    case "energy":
      v = range();
      break;
    case "duration":
      v = range(" min");
      break;
    case "key":
      v = f.a + (f.b === "+" ? ` ${t("filter.keyCompatible")}` : f.b === "++" ? ` ${t("filter.keyBoost")}` : "");
      break;
    case "rating":
      v = `${"★".repeat(Number(f.a))}+`;
      break;
    case "decision":
      v = t(f.a === "none" ? "filter.undecided" : (`decision.${f.a}` as MsgKey));
      break;
    case "added":
      v = t("filter.lastDays", { n: f.a });
      break;
    case "status":
      return (f.not ? `${t("filter.not")} ` : "") + t(`filter.status.${f.a.replace(":", "_")}` as MsgKey);
    default:
      v = f.a;
  }
  return `${f.not ? `${t("filter.not")} ` : ""}${name}: ${v}`;
}

function FilterEditor({ draft, onDraft, onSave, onCancel }: { draft: Filter; onDraft: (f: Filter) => void; onSave: () => void; onCancel: () => void }) {
  const { t } = useApp();
  const [tags, setTags] = useState<Tag[]>([]);
  const [genres, setGenres] = useState<string[]>([]);
  useEffect(() => {
    if (draft.kind === "tag") api.listTags().then(setTags, () => {});
    if (draft.kind === "genre") api.stats().then((s) => setGenres(s.genres.map((g) => g.label)), () => {});
  }, [draft.kind]);
  const set = (p: Partial<Filter>) => onDraft({ ...draft, ...p });
  const range = RANGE[draft.kind];

  let body: React.ReactNode;
  if (range) {
    const input = (key: "a" | "b", label: MsgKey) => (
      <label className="filter-field">
        <span>{t(label)}</span>
        <input
          type="number"
          inputMode="decimal"
          min={range.min}
          max={range.max}
          step={range.step}
          value={draft[key]}
          onChange={(e) => set({ [key]: e.target.value })}
          autoFocus={key === "a"}
        />
      </label>
    );
    body = (
      <div className="row">
        {input("a", "filter.from")}
        {input("b", "filter.to")}
        {draft.kind === "duration" && <span className="muted">min</span>}
      </div>
    );
  } else if (draft.kind === "key") {
    body = (
      <>
        <div className="key-grid">
          {KEYS.map((k) => (
            <button key={k} className={`key-cell ${draft.a === k ? "active" : ""}`} onClick={() => set({ a: k })}>
              {k}
            </button>
          ))}
        </div>
        <div className="seg">
          {(["", "+", "++"] as const).map((m) => (
            <button key={m} className={draft.b === m ? "active" : ""} onClick={() => set({ b: m })}>
              {t(m === "" ? "filter.keyExact" : m === "+" ? "filter.keyCompatible" : "filter.keyBoost")}
            </button>
          ))}
        </div>
      </>
    );
  } else if (draft.kind === "tag") {
    body = (
      <select value={draft.a} onChange={(e) => set({ a: e.target.value })} autoFocus>
        <option value="">—</option>
        {tags.map((g) => (
          <option key={g.id} value={g.name}>
            {g.name}
          </option>
        ))}
      </select>
    );
  } else if (draft.kind === "rating") {
    body = (
      <div className="seg">
        {["1", "2", "3", "4", "5"].map((n) => (
          <button key={n} className={draft.a === n ? "active" : ""} onClick={() => set({ a: n })}>
            {"★".repeat(Number(n))}
          </button>
        ))}
      </div>
    );
  } else if (draft.kind === "decision") {
    body = (
      <div className="seg">
        {["keep", "maybe", "pass", "none"].map((d) => (
          <button key={d} className={draft.a === d ? "active" : ""} onClick={() => set({ a: d })}>
            {t(d === "none" ? "filter.undecided" : (`decision.${d}` as MsgKey))}
          </button>
        ))}
      </div>
    );
  } else if (draft.kind === "added") {
    body = (
      <div className="seg">
        {["1", "7", "30", "90", "365"].map((d) => (
          <button key={d} className={draft.a === d ? "active" : ""} onClick={() => set({ a: d })}>
            {t("filter.lastDays", { n: d })}
          </button>
        ))}
      </div>
    );
  } else if (draft.kind === "status") {
    body = (
      <select value={draft.a} onChange={(e) => set({ a: e.target.value })} autoFocus>
        {STATUS.map((s) => (
          <option key={s} value={s}>
            {t(`filter.status.${s.replace(":", "_")}` as MsgKey)}
          </option>
        ))}
      </select>
    );
  } else {
    // genre, artist, label: free text, with the library's genres as suggestions
    body = (
      <>
        <input
          value={draft.a}
          onChange={(e) => set({ a: e.target.value })}
          list={draft.kind === "genre" ? "filter-genres" : undefined}
          placeholder={t("filter.contains")}
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && onSave()}
        />
        {draft.kind === "genre" && (
          <datalist id="filter-genres">
            {genres.map((g) => (
              <option key={g} value={g} />
            ))}
          </datalist>
        )}
        <p className="hint">{t("filter.textHint")}</p>
      </>
    );
  }

  return (
    <div className="filter-editor" role="dialog" aria-label={t(`filter.kind.${draft.kind}` as MsgKey)}>
      <div className="section-title">{t(`filter.kind.${draft.kind}` as MsgKey)}</div>
      {body}
      <label className="toggle">
        <input type="checkbox" checked={draft.not} onChange={(e) => set({ not: e.target.checked })} />
        {t("filter.exclude")}
      </label>
      <div className="row end">
        <button className="btn ghost small" onClick={onCancel}>
          {t("action.cancel")}
        </button>
        <button className="btn small" disabled={!filterQuery(draft)} onClick={onSave}>
          {t("filter.apply")}
        </button>
      </div>
    </div>
  );
}
