import { describe, expect, it } from "vitest";
import { camelotHue, formatBpm, formatBytes, formatDuration, formatLongDuration, quoteQueryValue, sourceLabel, trackLabel } from "./format";

describe("format", () => {
  it("durations", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(61_000)).toBe("1:01");
    expect(formatDuration(3_725_000)).toBe("1:02:05");
    expect(formatDuration(null)).toBe("");
    expect(formatDuration(-5)).toBe("");
    expect(formatLongDuration(90 * 60_000)).toBe("1h 30m");
    expect(formatLongDuration(3 * 24 * 3600_000 + 2 * 3600_000)).toBe("3d 2h");
  });
  it("bpm", () => {
    expect(formatBpm(124)).toBe("124");
    expect(formatBpm(123.5)).toBe("123.5");
    expect(formatBpm(123.45)).toBe("123.45");
    expect(formatBpm(null)).toBe("");
  });
  it("bytes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });
  it("camelot hues are distinct per wheel position and stable across A/B", () => {
    const hues = Array.from({ length: 12 }, (_, i) => camelotHue(`${i + 1}A`));
    expect(new Set(hues).size).toBe(12);
    expect(camelotHue("8A")).toBe(camelotHue("8B"));
    expect(camelotHue("13A")).toBeNull();
    expect(camelotHue("Am")).toBeNull();
  });
  it("labels", () => {
    expect(trackLabel({ artist: "A", title: "B" })).toBe("A — B");
    expect(trackLabel({ artist: null, title: "B" })).toBe("B");
    expect(sourceLabel("import:rekordbox")).toBe("rekordbox");
    expect(quoteQueryValue("tech house")).toBe('"tech house"');
    expect(quoteQueryValue("techno")).toBe("techno");
  });
});

import { fillBuckets } from "./views/Dashboard";
describe("histogram buckets", () => {
  it("fills numeric gaps and leaves categories alone", () => {
    expect(fillBuckets([{ label: "120", count: 2 }, { label: "126", count: 1 }], 2).map((b) => b.count)).toEqual([2, 0, 0, 1]);
    expect(fillBuckets([{ label: "-12", count: 1 }, { label: "-10", count: 3 }], 1).map((b) => b.label)).toEqual(["-12", "-11", "-10"]);
    const cats = [{ label: "MP3", count: 1 }];
    expect(fillBuckets(cats, 1)).toBe(cats);
  });
});
