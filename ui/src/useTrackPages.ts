// Paged, cached loading of a track query for the virtualised table.
import { useCallback, useEffect, useRef, useState } from "react";
import { api, type TrackQuery, type TrackRow } from "./api";

export const PAGE = 200;

export interface TrackPages {
  total: number;
  loading: boolean;
  error: string | null;
  rowAt: (i: number) => TrackRow | undefined;
  ensure: (start: number, end: number) => void;
  /** Patch a loaded row in place (optimistic UI). */
  patch: (id: string, change: Partial<TrackRow>) => void;
  version: number;
}

export function useTrackPages(query: TrackQuery, dataVersion: number): TrackPages {
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const pages = useRef(new Map<number, TrackRow[]>());
  const inflight = useRef(new Set<number>());
  const generation = useRef(0);
  const key = JSON.stringify(query);

  const load = useCallback(
    (page: number, gen: number) => {
      if (inflight.current.has(page)) return;
      inflight.current.add(page);
      setLoading(true);
      api
        .queryTracks({ ...query, offset: page * PAGE, limit: PAGE })
        .then((res) => {
          if (gen !== generation.current) return;
          pages.current.set(page, res.tracks);
          setTotal(res.total);
          setError(null);
          setVersion((v) => v + 1);
        })
        .catch((e) => {
          if (gen === generation.current) setError(String(e));
        })
        .finally(() => {
          inflight.current.delete(page);
          if (gen === generation.current) setLoading(false);
        });
    },
    [key],
  );

  // New query or changed data: start over (keep showing old rows until
  // the first page arrives to avoid flicker).
  useEffect(() => {
    generation.current++;
    const gen = generation.current;
    const keep = pages.current;
    pages.current = new Map();
    inflight.current = new Set();
    // Reload pages that were visible so scrolling position stays meaningful.
    const visible = [...keep.keys()];
    if (visible.length === 0) visible.push(0);
    for (const p of visible.slice(0, 3)) load(p, gen);
  }, [key, dataVersion, load]);

  const rowAt = useCallback((i: number) => pages.current.get(Math.floor(i / PAGE))?.[i % PAGE], []);

  const ensure = useCallback(
    (start: number, end: number) => {
      const gen = generation.current;
      for (let p = Math.floor(Math.max(0, start) / PAGE); p <= Math.floor(Math.max(0, end) / PAGE); p++) {
        if (!pages.current.has(p)) load(p, gen);
      }
    },
    [load],
  );

  const patch = useCallback((id: string, change: Partial<TrackRow>) => {
    for (const rows of pages.current.values()) {
      const i = rows.findIndex((r) => r.id === id);
      if (i >= 0) rows[i] = { ...rows[i], ...change };
    }
    setVersion((v) => v + 1);
  }, []);

  return { total, loading, error, rowAt, ensure, patch, version };
}
