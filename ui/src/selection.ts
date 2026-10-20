// Multi-selection logic for the track table (pure, unit-tested).

export interface Selection {
  ids: Set<string>;
  /** Anchor index for shift-range selection. */
  anchor: number | null;
  /** Focused row index (keyboard cursor). */
  focus: number | null;
}

export const emptySelection = (): Selection => ({ ids: new Set(), anchor: null, focus: null });

export interface ClickMods {
  shift: boolean;
  toggle: boolean; // ctrl on Windows/Linux, cmd on macOS
}

/**
 * Apply a click on row `index` (with id `id`). `idAt` resolves ids of other
 * rows for range selection; rows not loaded yet return undefined and are
 * skipped.
 */
export function clickRow(sel: Selection, index: number, id: string, mods: ClickMods, idAt: (i: number) => string | undefined): Selection {
  if (mods.shift && sel.anchor != null) {
    const [a, b] = sel.anchor < index ? [sel.anchor, index] : [index, sel.anchor];
    const ids = mods.toggle ? new Set(sel.ids) : new Set<string>();
    for (let i = a; i <= b; i++) {
      const rid = idAt(i);
      if (rid) ids.add(rid);
    }
    return { ids, anchor: sel.anchor, focus: index };
  }
  if (mods.toggle) {
    const ids = new Set(sel.ids);
    if (ids.has(id)) ids.delete(id);
    else ids.add(id);
    return { ids, anchor: index, focus: index };
  }
  return { ids: new Set([id]), anchor: index, focus: index };
}

/** Keyboard move: returns a single-row selection at the new index. */
export function moveFocus(sel: Selection, delta: number, total: number, idAt: (i: number) => string | undefined, extend = false): Selection {
  if (total === 0) return sel;
  const from = sel.focus ?? (delta > 0 ? -1 : total);
  const to = Math.max(0, Math.min(total - 1, from + delta));
  const id = idAt(to);
  if (!id) return { ...sel, focus: to };
  if (extend && sel.anchor != null) return clickRow(sel, to, id, { shift: true, toggle: false }, idAt);
  return { ids: new Set([id]), anchor: to, focus: to };
}
