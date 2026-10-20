import { describe, expect, it } from "vitest";
import { _dicts, translate } from "./i18n";

describe("i18n", () => {
  it("italian covers every english key with a non-empty string", () => {
    for (const k of Object.keys(_dicts.en)) {
      expect(typeof _dicts.it[k as keyof typeof _dicts.it], k).toBe("string");
      expect(_dicts.it[k as keyof typeof _dicts.it].length, k).toBeGreaterThan(0);
    }
    expect(Object.keys(_dicts.it).sort()).toEqual(Object.keys(_dicts.en).sort());
  });
  it("placeholders are the same in both languages", () => {
    const ph = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join(",");
    for (const k of Object.keys(_dicts.en) as (keyof typeof _dicts.en)[]) {
      expect(ph(_dicts.it[k]), k).toBe(ph(_dicts.en[k]));
    }
  });
  it("interpolates", () => {
    expect(translate("en", "details.multi", { n: 3 })).toBe("3 tracks selected");
    expect(translate("it", "details.multi", { n: 3 })).toBe("3 tracce selezionate");
  });
});
