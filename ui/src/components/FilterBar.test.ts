import { describe, expect, it } from "vitest";
import { filterQuery, filtersQuery, type Filter } from "./FilterBar";

const f = (kind: Filter["kind"], a: string, b = "", not = false): Filter => ({ kind, a, b, not });

describe("filter bubbles", () => {
  it("compile to the query language", () => {
    expect(filterQuery(f("bpm", "120", "126"))).toBe("bpm:120-126");
    expect(filterQuery(f("bpm", "120", ""))).toBe("bpm:>=120");
    expect(filterQuery(f("year", "", "1999"))).toBe("year:<=1999");
    expect(filterQuery(f("duration", "5", "7"))).toBe("duration:300-420");
    expect(filterQuery(f("key", "8A", "+"))).toBe("key:8A+");
    expect(filterQuery(f("genre", "Tech House"))).toBe('genre:"Tech House"');
    expect(filterQuery(f("artist", "kolsch"))).toBe("artist:kolsch");
    expect(filterQuery(f("rating", "4"))).toBe("rating:>=4");
    expect(filterQuery(f("decision", "none"))).toBe("decision:none");
    expect(filterQuery(f("added", "7"))).toBe("added:7d");
    expect(filterQuery(f("status", "missing:yes", "", true))).toBe("-missing:yes");
  });
  it("skip empty bubbles and join with the typed text", () => {
    expect(filterQuery(f("bpm", "", ""))).toBe("");
    expect(filtersQuery(" rolling ", [f("bpm", "", ""), f("tag", "warm up")])).toBe('rolling tag:"warm up"');
  });
});
