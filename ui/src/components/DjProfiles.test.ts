import { describe, expect, it } from "vitest";
import { DEFAULT_WEIGHTS } from "../api";
import { _dicts } from "../i18n";
import { DJ_PROFILES } from "./DjProfiles";

describe("DJ profiles", () => {
  it("only use values the app accepts", () => {
    for (const p of DJ_PROFILES) {
      if (p.bpmRange !== "auto") {
        // Same rule as the analyser's parse_range: 40–250 and a range of at least 1.5×.
        const [lo, hi] = p.bpmRange.split("-").map(Number);
        expect(lo).toBeGreaterThanOrEqual(40);
        expect(hi).toBeLessThanOrEqual(250);
        expect(hi).toBeGreaterThanOrEqual(lo * 1.5);
      }
      expect([0.5, 0.6, 0.7, 0.8]).toContain(p.genreMinAgreement);
      for (const k of Object.keys(p.weights)) expect(Object.keys(DEFAULT_WEIGHTS)).toContain(k);
      for (const lang of ["en", "it"] as const) {
        const d = _dicts[lang] as Record<string, string>;
        expect(d[`profiles.${p.id}`]).toBeTruthy();
        expect(d[`profiles.${p.id}.desc`]).toBeTruthy();
      }
    }
  });
});
