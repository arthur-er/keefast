import { describe, expect, it } from "vitest";

import { searchSections, type SearchableSection } from "./search";
import { CLIPBOARD_WAITS, describeWait, IDLE_LOCK_WAITS, waitsWith } from "./waits";

describe("waits", () => {
  it("read in the largest whole unit", () => {
    expect(describeWait(30)).toBe("30 s");
    expect(describeWait(90)).toBe("90 s");
    expect(describeWait(120)).toBe("2 min");
    expect(describeWait(90 * 60)).toBe("90 min");
    expect(describeWait(4 * 3600)).toBe("4 h");
    expect(describeWait(null)).toBe("Never");
  });

  it("offer a wait set by hand in its place", () => {
    const offered = [10, 60, null];
    expect(waitsWith(offered, 60)).toEqual([10, 60, null]);
    expect(waitsWith(offered, 45)).toEqual([10, 45, 60, null]);
    expect(waitsWith(offered, 3600)).toEqual([10, 60, 3600, null]);
    expect(waitsWith(offered, 5)).toEqual([5, 10, 60, null]);
  });

  it("offer what the GPUI settings offered", () => {
    expect(IDLE_LOCK_WAITS.map(describeWait)).toEqual(["1 min", "5 min", "15 min", "30 min", "1 h", "4 h", "Never"]);
    expect(CLIPBOARD_WAITS.map(describeWait)).toEqual(["10 s", "20 s", "30 s", "1 min", "2 min", "5 min", "Never"]);
  });
});

describe("searching settings", () => {
  const sections: SearchableSection[] = [
    { id: "locking", name: "Locking", desc: "When unlocked Vaults lock themselves.", cards: [[{ id: "idle", search: "Lock when idle" }, { id: "os", search: "Lock with the screen" }]] },
    { id: "clipboard", name: "Clipboard", cards: [[{ id: "clear", search: "Clear clipboard copy paste" }]] },
    { id: "browser", name: "Browser", cards: [[{ id: "integration", search: "Browser integration" }], [{ id: "pairings", search: "Pairings" }]] },
    { id: "key", name: "Security", cards: [[{ id: "form", search: "Master key", pinned: true }]] },
  ];

  it("shows everything for an empty query", () => {
    const result = searchSections(sections, "  ");
    expect(result.empty).toBe(false);
    expect(result.sections.map((s) => s.cards.flat().length)).toEqual([2, 1, 2, 1]);
  });

  it("shows a section whole when its title matches", () => {
    const result = searchSections(sections, "unlocked");
    expect(result.sections[0]!.cards.flat().map((i) => i.id)).toEqual(["idle", "os"]);
    expect(result.sections[0]!.count).toBe(1);
  });

  it("keeps only matching items, and counts them", () => {
    const result = searchSections(sections, "PASTE");
    expect(result.sections[1]!.cards.flat().map((i) => i.id)).toEqual(["clear"]);
    expect(result.sections[1]!.count).toBe(1);
    expect(result.sections[0]!.count).toBe(0);
    expect(result.sections[0]!.cards).toEqual([]);
  });

  it("drops cards left empty", () => {
    const result = searchSections(sections, "pairings");
    expect(result.sections[2]!.cards).toEqual([[{ id: "pairings", search: "Pairings" }]]);
  });

  it("never hides a pinned item", () => {
    const result = searchSections(sections, "clipboard");
    expect(result.sections[3]!.count).toBe(1);
    expect(result.sections[3]!.cards.flat().map((i) => i.id)).toEqual(["form"]);
  });

  it("says when nothing matches", () => {
    expect(searchSections(sections.slice(0, 3), "zzz").empty).toBe(true);
  });
});
