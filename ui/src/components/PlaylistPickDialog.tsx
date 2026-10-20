// Pick other playlists for merge / common / subtract, and name the result.
import { useState } from "react";
import type { Playlist } from "../api";
import { useApp } from "../store";

export type CombineOp = "union" | "intersect" | "subtract";

export function PlaylistPickDialog({
  base,
  playlists,
  op,
  onConfirm,
  onClose,
}: {
  base: Playlist;
  playlists: Playlist[];
  op: CombineOp;
  onConfirm: (name: string, others: string[]) => void;
  onClose: () => void;
}) {
  const { t } = useApp();
  const [picked, setPicked] = useState<string[]>([]);
  const [name, setName] = useState(`${base.name} · ${t(`pl.op.${op}` as never)}`);
  const candidates = playlists.filter((p) => p.id !== base.id && (p.kind === "manual" || p.kind === "set"));
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" role="dialog" aria-modal onMouseDown={(e) => e.stopPropagation()}>
        <h3>{t(`pl.title.${op}` as never, { name: base.name })}</h3>
        <p className="hint">{t(`pl.hint.${op}` as never)}</p>
        <div className="pick-list">
          {candidates.map((p) => (
            <label key={p.id} className="toggle">
              <input
                type="checkbox"
                checked={picked.includes(p.id)}
                onChange={(e) => setPicked(e.target.checked ? [...picked, p.id] : picked.filter((x) => x !== p.id))}
              />
              {p.name} <span className="muted small">({p.trackCount})</span>
            </label>
          ))}
          {candidates.length === 0 && <p className="muted">{t("pl.noOthers")}</p>}
        </div>
        <label className="field">
          <span>{t("pl.newName")}</span>
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="row end">
          <button className="btn ghost" onClick={onClose}>
            {t("action.cancel")}
          </button>
          <button className="btn" disabled={picked.length === 0 || !name.trim()} onClick={() => onConfirm(name.trim(), picked)}>
            {t("pl.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
