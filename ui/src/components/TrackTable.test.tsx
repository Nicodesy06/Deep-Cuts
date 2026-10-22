import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TrackRow } from "../api";
import type { TrackPages } from "../useTrackPages";
import { ALL_COLUMNS, TrackTable } from "./TrackTable";

const row = (i: number): TrackRow => ({
  id: `id-${i}`, title: `Title ${i}`, artist: `Artist ${i}`, version: null, remixer: null, album: null, label: null,
  genre: "House", year: 2020, bpm: 124, key: "Am", camelot: "8A", durationMs: 360000, rating: 3, favorite: false,
  decision: i % 2 ? "keep" : null, comment: null, path: `/m/${i}.mp3`, format: "mp3", bitrate: 320, missing: false,
  lufs: -8.2, previewMs: 60000, analysisStatus: "done", createdAt: 0, energy: 7, subgenre: null, tags: ["warmup"], cues: 2,
});

describe("TrackTable", () => {
  it("renders only the visible window of a large library", () => {
    globalThis.ResizeObserver = class { observe() {} disconnect() {} unobserve() {} } as unknown as typeof ResizeObserver;
    const pages: TrackPages = {
      total: 100_000, loading: false, error: null, version: 1,
      rowAt: (i) => row(i), ensure: vi.fn(), patch: vi.fn(),
    };
    const { container } = render(
      <TrackTable pages={pages} columns={ALL_COLUMNS.slice(0, 6)} t={(k) => k} sort={null} desc={false} onSort={() => {}}
        selected={new Set(["id-1"])} focus={1} playingId={null} onRowClick={() => {}} onRowDoubleClick={() => {}}
        onRowContext={() => {}} empty={<span>empty</span>} />,
    );
    const rows = container.querySelectorAll(".tr");
    expect(rows.length).toBeGreaterThan(5);
    expect(rows.length).toBeLessThan(100);
    expect(container.querySelector(".tr-sel")?.textContent).toContain("Title 1");
    expect(pages.ensure).toHaveBeenCalled();
  });
  it("shows the empty state", () => {
    const pages: TrackPages = { total: 0, loading: false, error: null, version: 1, rowAt: () => undefined, ensure: vi.fn(), patch: vi.fn() };
    const { getByText } = render(
      <TrackTable pages={pages} columns={ALL_COLUMNS} t={(k) => k} sort={null} desc={false} onSort={() => {}} selected={new Set()}
        focus={null} playingId={null} onRowClick={() => {}} onRowDoubleClick={() => {}} onRowContext={() => {}} empty={<span>empty!</span>} />,
    );
    expect(getByText("empty!")).toBeTruthy();
  });
});

import { effectiveGrid, tapBpm } from "./GridEditor";

describe("grid helpers", () => {
  it("prefers the user grid, then imported, then analysis", () => {
    const g = effectiveGrid([
      { source: "analysis", bpm: 120, firstBeatMs: 0 },
      { source: "import:rekordbox", bpm: 121, firstBeatMs: 0 },
      { source: "user", bpm: 122, firstBeatMs: 0 },
    ]);
    expect(g?.bpm).toBe(122);
    expect(effectiveGrid([{ source: "analysis", bpm: 120, firstBeatMs: 0 }, { source: "import:traktor", bpm: 121, firstBeatMs: 0 }])?.bpm).toBe(121);
    expect(effectiveGrid([])).toBeNull();
  });
  it("tap tempo uses the median interval", () => {
    expect(tapBpm([0, 500, 1000])).toBeNull();
    expect(tapBpm([0, 500, 1000, 1500, 2000])).toBe(120);
    expect(tapBpm([0, 469, 938, 1500, 1875, 2344])).toBe(127.9);
  });
});

import { historyValue } from "./TrackExtras";
describe("change history values", () => {
  it("shows cues instead of JSON", () => {
    expect(historyValue('{"kind":"loop","slot":null,"startMs":14806.9,"endMs":22191.5,"name":"Outro loop","locked":true}')).toBe("loop “Outro loop” 0:15–0:22");
    expect(historyValue('{"kind":"hotcue","slot":0,"startMs":1000}')).toBe("hotcue A 0:01");
    expect(historyValue("Tech House")).toBe("Tech House");
    expect(historyValue(null)).toBe("∅");
  });
});
