// Virtualised track table: fixed row height, only visible rows rendered.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { TrackRow } from "../api";
import { camelotHue, formatBpm, formatDuration } from "../format";
import type { MsgKey } from "../i18n";
import type { TrackPages } from "../useTrackPages";

export const ROW_H = 34;

export interface Column {
  id: string;
  label: MsgKey;
  width: number;
  sort?: string;
  align?: "right" | "center";
  render: (t: TrackRow) => ReactNode;
}

export const DecisionDot = ({ d }: { d: TrackRow["decision"] }) =>
  d ? <span className={`dec-dot dec-${d}`} title={d} /> : <span className="dec-dot dec-none" />;

export const Stars = ({ n }: { n: number | null }) => (
  <span className="stars" aria-label={`${n ?? 0}/5`}>
    {"★★★★★".slice(0, n ?? 0)}
    <span className="stars-off">{"★★★★★".slice(n ?? 0)}</span>
  </span>
);

export const KeyChip = ({ t }: { t: TrackRow }) => {
  const hue = camelotHue(t.camelot);
  if (!t.camelot && !t.key) return null;
  return (
    <span className="key-chip" style={hue != null ? { ["--kh" as string]: hue } : undefined} title={t.key ?? ""}>
      {t.camelot ?? t.key}
    </span>
  );
};

export const ALL_COLUMNS: Column[] = [
  { id: "decision", label: "col.decision", width: 34, sort: "decision", align: "center", render: (t) => <DecisionDot d={t.decision} /> },
  { id: "artist", label: "col.artist", width: 160, sort: "artist", render: (t) => t.artist },
  {
    id: "title",
    label: "col.title",
    width: 250,
    sort: "title",
    render: (t) => (
      <>
        {t.favorite && <span className="fav">♥ </span>}
        {t.title}
        {t.missing && <span className="badge badge-warn">missing</span>}
      </>
    ),
  },
  { id: "version", label: "col.version", width: 130, render: (t) => t.version },
  { id: "remixer", label: "col.remixer", width: 120, render: (t) => t.remixer },
  { id: "bpm", label: "col.bpm", width: 70, sort: "bpm", align: "right", render: (t) => <span className="num">{formatBpm(t.bpm)}</span> },
  { id: "key", label: "col.key", width: 58, sort: "key", align: "center", render: (t) => <KeyChip t={t} /> },
  { id: "duration", label: "col.duration", width: 64, sort: "durationMs", align: "right", render: (t) => <span className="num">{formatDuration(t.durationMs)}</span> },
  { id: "genre", label: "col.genre", width: 120, sort: "genre", render: (t) => t.genre },
  { id: "subgenre", label: "col.subgenre", width: 120, sort: "subgenre", render: (t) => t.subgenre },
  {
    id: "tags",
    label: "col.tags",
    width: 200,
    render: (t) => (
      <span className="tag-cell" title={t.tags.join(", ")}>
        {t.tags.map((g) => (
          <span key={g} className="tag-mini">
            {g}
          </span>
        ))}
      </span>
    ),
  },
  { id: "label", label: "col.label", width: 130, sort: "label", render: (t) => t.label },
  { id: "album", label: "col.album", width: 150, sort: "album", render: (t) => t.album },
  { id: "year", label: "col.year", width: 56, sort: "year", align: "right", render: (t) => <span className="num">{t.year ?? ""}</span> },
  { id: "rating", label: "col.rating", width: 100, sort: "rating", render: (t) => <Stars n={t.rating} /> },
  { id: "energy", label: "col.energy", width: 74, sort: "energy", align: "right", render: (t) => <span className="num">{t.energy ?? ""}</span> },
  { id: "fav", label: "col.fav", width: 34, align: "center", render: (t) => (t.favorite ? <span className="fav-on">♥</span> : null) },
  { id: "cues", label: "col.cues", width: 50, align: "right", render: (t) => <span className="num muted">{t.cues || ""}</span> },
  { id: "lufs", label: "col.lufs", width: 64, sort: "lufs", align: "right", render: (t) => <span className="num">{t.lufs != null ? t.lufs.toFixed(1) : ""}</span> },
  { id: "format", label: "col.format", width: 60, render: (t) => <span className="muted">{t.format?.toUpperCase()}</span> },
  { id: "bitrate", label: "col.bitrate", width: 64, align: "right", render: (t) => <span className="num muted">{t.bitrate ?? ""}</span> },
  {
    id: "analysis",
    label: "col.analysis",
    width: 72,
    render: (t) => <span className={`an an-${t.analysisStatus}`}>{t.analysisStatus === "none" ? "" : t.analysisStatus}</span>,
  },
  { id: "added", label: "col.added", width: 92, sort: "createdAt", render: (t) => <span className="num muted">{new Date(t.createdAt).toLocaleDateString()}</span> },
  { id: "comment", label: "col.comment", width: 180, render: (t) => <span className="muted">{t.comment}</span> },
  { id: "path", label: "col.path", width: 260, render: (t) => <span className="muted">{t.path}</span> },
];

export const DEFAULT_COLUMNS = ["decision", "artist", "title", "bpm", "key", "duration", "genre", "rating"];

interface Props {
  pages: TrackPages;
  columns: Column[];
  t: (k: MsgKey) => string;
  sort: string | null;
  desc: boolean;
  onSort: (sort: string) => void;
  selected: Set<string>;
  focus: number | null;
  playingId: string | null;
  onRowClick: (index: number, row: TrackRow, e: React.MouseEvent) => void;
  onRowDoubleClick: (index: number, row: TrackRow) => void;
  onRowContext: (index: number, row: TrackRow, e: React.MouseEvent) => void;
  onDragStart?: (row: TrackRow, e: React.DragEvent) => void;
  /** Shows a discreet "+" at the end of the header to add columns. */
  onAddColumn?: () => void;
  empty: ReactNode;
}

export function TrackTable(p: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(600);
  const { total, rowAt, ensure } = p.pages;

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    setHeight(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  const first = Math.max(0, Math.floor(scrollTop / ROW_H) - 8);
  const last = Math.min(total - 1, Math.ceil((scrollTop + height) / ROW_H) + 8);
  useEffect(() => {
    if (total > 0) ensure(first, last);
  }, [first, last, total, ensure]);

  // Keep the keyboard focus row in view.
  useEffect(() => {
    const el = scroller.current;
    if (!el || p.focus == null) return;
    const top = p.focus * ROW_H;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + 2 * ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = top + 2 * ROW_H - el.clientHeight;
  }, [p.focus]);

  // Room for the "+" after the last column.
  const width = p.columns.reduce((s, c) => s + c.width, 0) + (p.onAddColumn ? 34 : 0);
  const rows: ReactNode[] = [];
  for (let i = first; i <= last; i++) {
    const r = rowAt(i);
    // Rows span the whole view even when the columns are narrower than it.
    const style = { top: i * ROW_H, height: ROW_H, width: `max(${width}px, 100%)` };
    if (!r) {
      rows.push(<div key={`ph${i}`} className="tr tr-placeholder" style={style} />);
      continue;
    }
    const cls = ["tr", p.selected.has(r.id) && "tr-sel", p.focus === i && "tr-focus", p.playingId === r.id && "tr-playing", r.missing && "tr-missing"]
      .filter(Boolean)
      .join(" ");
    rows.push(
      <div
        key={r.id + i}
        className={cls}
        style={style}
        role="row"
        aria-selected={p.selected.has(r.id)}
        draggable={!!p.onDragStart}
        onDragStart={(e) => p.onDragStart?.(r, e)}
        onMouseDown={(e) => e.button === 0 && p.onRowClick(i, r, e)}
        onDoubleClick={() => p.onRowDoubleClick(i, r)}
        onContextMenu={(e) => {
          e.preventDefault();
          p.onRowContext(i, r, e);
        }}
      >
        {p.columns.map((c) => (
          <div key={c.id} className={`td ${c.align ? `td-${c.align}` : ""}`} style={{ width: c.width }}>
            {c.render(r)}
          </div>
        ))}
      </div>,
    );
  }

  return (
    <div className="table" role="grid" aria-rowcount={total}>
      <div className="tbody" ref={scroller} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        <div className="thead" style={{ width: `max(${width}px, 100%)` }}>
          {p.columns.map((c) => (
            <div
              key={c.id}
              className={`th ${c.sort ? "th-sortable" : ""} ${c.align ? `td-${c.align}` : ""} ${p.sort === c.sort && c.sort ? "th-active" : ""}`}
              style={{ width: c.width }}
              onClick={() => c.sort && p.onSort(c.sort)}
              title={p.t(c.label)}
            >
              {c.id === "decision" ? "●" : p.t(c.label)}
              {p.sort === c.sort && c.sort && <span className="sort-arrow">{p.desc ? "▾" : "▴"}</span>}
            </div>
          ))}
          {p.onAddColumn && (
            <button className="th-add" onClick={p.onAddColumn} title={p.t("library.addColumn")} aria-label={p.t("library.addColumn")}>
              +
            </button>
          )}
        </div>
        {total === 0 && !p.pages.loading ? (
          <div className="table-empty">{p.empty}</div>
        ) : (
          <div className="tbody-inner" style={{ height: total * ROW_H, width: `max(${width}px, 100%)` }}>
            {rows}
          </div>
        )}
      </div>
    </div>
  );
}
