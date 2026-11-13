import { describe, expect, it } from "vitest";

import type { EntrySummary, PaletteMatch } from "@/bindings";

import { highlightRuns, isConfident, orderSections, rankByTitle, type Ranked } from "./rank";

const found = (score: number, positions: number[] = []): PaletteMatch => ({ score, positions });
const entry = (title: string, score: number, positions: number[]): Ranked<EntrySummary> => ({ item: { title } as EntrySummary, score, positions });

describe("rankByTitle", () => {
  const commands = [{ title: "Copy username" }, { title: "Copy password" }, { title: "Save all Vaults" }];
  const byName = (matches: (PaletteMatch | null)[]) => new Map(commands.map((command, index) => [command.title, matches[index] ?? null]));

  it("puts the best match first and drops the others", () => {
    const ranked = rankByTitle(commands, byName([found(30), found(33, [0, 5]), null]));
    expect(ranked.map((r) => r.item.title)).toEqual(["Copy password", "Copy username"]);
    expect(ranked[0]?.positions).toEqual([0, 5]);
  });

  it("keeps the order of equal matches", () => {
    expect(rankByTitle(commands, byName([found(33), found(33), null])).map((r) => r.item.title)).toEqual(["Copy username", "Copy password"]);
  });

  it("drops titles it has no match for", () => {
    expect(rankByTitle(commands, new Map([["Save all Vaults", found(33)]])).map((r) => r.item.title)).toEqual(["Save all Vaults"]);
  });
});

describe("isConfident", () => {
  it("is confident when one title match stands out", () => {
    expect(isConfident([entry("GitHub", 33, [0, 1, 2, 3])])).toBe(true);
    expect(isConfident([entry("GitHub", 33, [0]), entry("Google", 30, [0])])).toBe(true);
    expect(isConfident([entry("GitHub", 33, [0]), entry("Google", 33, [0])])).toBe(false);
    // Found by its username, not its title.
    expect(isConfident([entry("Stripe", 28, [])])).toBe(false);
    expect(isConfident([])).toBe(false);
  });
});

describe("highlightRuns", () => {
  it("merges adjacent matched characters", () => {
    expect(highlightRuns("Copy Password", [0, 5, 6])).toEqual([
      { text: "C", matched: true },
      { text: "opy ", matched: false },
      { text: "Pa", matched: true },
      { text: "ssword", matched: false },
    ]);
  });

  it("counts positions in UTF-16 code units", () => {
    expect(highlightRuns("🔑 Key", [3])).toEqual([
      { text: "🔑 ", matched: false },
      { text: "K", matched: true },
      { text: "ey", matched: false },
    ]);
  });
});

describe("orderSections", () => {
  it("puts the Entry's commands first and unknown sections before App", () => {
    expect(orderSections(["Go to", "Vaults", "Settings", "Entry", "App", "Entry"])).toEqual(["Entry", "Vaults", "Go to", "Settings", "App"]);
  });
});
