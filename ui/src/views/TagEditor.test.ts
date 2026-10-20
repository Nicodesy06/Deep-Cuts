import { describe, expect, it } from "vitest";
import type { Tag } from "../api";
import { tagTree } from "./TagEditor";

const tag = (id: string, name: string, parentId: string | null = null): Tag => ({
  id, name, parentId, category: "custom", trackCount: 0, quickSlot: null, synonyms: [],
});

describe("tagTree", () => {
  it("orders parents before children with depth, alphabetically", () => {
    const tree = tagTree([tag("c", "Early", "w"), tag("w", "Warm-up"), tag("a", "Afters"), tag("d", "Deep", "c")]);
    expect(tree.map((n) => [n.tag.name, n.depth])).toEqual([["Afters", 0], ["Warm-up", 0], ["Early", 1], ["Deep", 2]]);
  });
  it("treats a missing parent as top level and never loops", () => {
    const tree = tagTree([tag("x", "Orphan", "gone"), tag("p", "P", "q"), tag("q", "Q", "p")]);
    expect(tree.map((n) => n.tag.name)).toEqual(["Orphan"]);
  });
});
