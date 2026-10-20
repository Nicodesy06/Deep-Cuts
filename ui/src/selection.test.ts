import { describe, expect, it } from "vitest";
import { clickRow, emptySelection, moveFocus } from "./selection";

const ids = ["a", "b", "c", "d", "e"];
const idAt = (i: number) => ids[i];
const plain = { shift: false, toggle: false };

describe("selection", () => {
  it("click selects one row", () => {
    const s = clickRow(emptySelection(), 2, "c", plain, idAt);
    expect([...s.ids]).toEqual(["c"]);
    expect(s.anchor).toBe(2);
  });
  it("shift extends from the anchor in both directions", () => {
    let s = clickRow(emptySelection(), 1, "b", plain, idAt);
    s = clickRow(s, 3, "d", { shift: true, toggle: false }, idAt);
    expect([...s.ids].sort()).toEqual(["b", "c", "d"]);
    s = clickRow(s, 0, "a", { shift: true, toggle: false }, idAt);
    expect([...s.ids].sort()).toEqual(["a", "b"]);
  });
  it("toggle adds and removes", () => {
    let s = clickRow(emptySelection(), 0, "a", plain, idAt);
    s = clickRow(s, 4, "e", { shift: false, toggle: true }, idAt);
    expect([...s.ids].sort()).toEqual(["a", "e"]);
    s = clickRow(s, 0, "a", { shift: false, toggle: true }, idAt);
    expect([...s.ids]).toEqual(["e"]);
  });
  it("keyboard moves clamp to bounds and can extend", () => {
    let s = moveFocus(emptySelection(), 1, 5, idAt);
    expect(s.focus).toBe(0);
    s = moveFocus(s, -1, 5, idAt);
    expect(s.focus).toBe(0);
    s = moveFocus(s, 10, 5, idAt);
    expect([...s.ids]).toEqual(["e"]);
    s = moveFocus(s, -2, 5, idAt, true);
    expect([...s.ids].sort()).toEqual(["c", "d", "e"]);
  });
  it("skips rows not loaded yet", () => {
    const sparse = (i: number) => (i === 2 ? undefined : ids[i]);
    let s = clickRow(emptySelection(), 0, "a", plain, sparse);
    s = clickRow(s, 4, "e", { shift: true, toggle: false }, sparse);
    expect([...s.ids].sort()).toEqual(["a", "b", "d", "e"]);
  });
});
