// Single-series bar chart (magnitude): one validated hue, 2px gaps, rounded
// data ends on the baseline, per-bar hover tooltip, table fallback.
import { useState } from "react";
import type { Bucket } from "../api";

interface Props {
  title: string;
  data: Bucket[];
  /** Horizontal bars suit long category names (genres). */
  horizontal?: boolean;
  unit?: string;
  onSelect?: (b: Bucket) => void;
  emptyText: string;
  height?: number;
}

export function BarChart({ title, data, horizontal = false, unit = "", onSelect, emptyText, height = 160 }: Props) {
  const [hover, setHover] = useState<number | null>(null);
  const [asTable, setAsTable] = useState(false);
  const max = Math.max(1, ...data.map((d) => d.count));
  const total = data.reduce((s, d) => s + d.count, 0);

  return (
    <figure className="chart">
      <figcaption>
        <span>{title}</span>
        {data.length > 0 && (
          <button className="link-btn small" onClick={() => setAsTable((v) => !v)} aria-pressed={asTable}>
            {asTable ? "chart" : "table"}
          </button>
        )}
      </figcaption>
      {data.length === 0 ? (
        <div className="chart-empty muted">{emptyText}</div>
      ) : asTable ? (
        <table className="chart-table">
          <tbody>
            {data.map((d) => (
              <tr key={d.label}>
                <td>
                  {d.label}
                  {unit}
                </td>
                <td className="num">{d.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : horizontal ? (
        <div className="hbars">
          {data.map((d, i) => (
            <button
              key={d.label}
              className={`hbar-row ${hover === i ? "hover" : ""}`}
              onMouseEnter={() => setHover(i)}
              onMouseLeave={() => setHover(null)}
              onClick={() => onSelect?.(d)}
              title={`${d.label}: ${d.count} (${Math.round((d.count / total) * 100)}%)`}
            >
              <span className="hbar-label">{d.label}</span>
              <span className="hbar-track">
                <span className="hbar" style={{ width: `${(d.count / max) * 100}%` }} />
              </span>
              <span className="hbar-value num">{d.count}</span>
            </button>
          ))}
        </div>
      ) : (
        <div className="vbars-wrap">
          <div className="vbars" style={{ height }} onMouseLeave={() => setHover(null)}>
            {data.map((d, i) => (
              <button
                key={d.label}
                className={`vbar-hit ${hover === i ? "hover" : ""}`}
                onMouseEnter={() => setHover(i)}
                onFocus={() => setHover(i)}
                onClick={() => onSelect?.(d)}
                aria-label={`${d.label}${unit}: ${d.count}`}
              >
                <span className="vbar" style={{ height: `${Math.max(2, (d.count / max) * 100)}%` }} />
              </button>
            ))}
            {hover != null && data[hover] && (
              <div className="tooltip" style={{ left: `${((hover + 0.5) / data.length) * 100}%` }}>
                <strong className="num">{data[hover].count}</strong>
                <span>
                  {data[hover].label}
                  {unit}
                </span>
              </div>
            )}
          </div>
          <div className="vbars-axis">
            <span>
              {data[0].label}
              {unit}
            </span>
            <span>
              {data[data.length - 1].label}
              {unit}
            </span>
          </div>
        </div>
      )}
    </figure>
  );
}
