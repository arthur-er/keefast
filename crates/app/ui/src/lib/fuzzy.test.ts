// The same cases as keefast_core::fuzzy_match's tests, so both rank alike.

import { describe, expect, it } from "vitest";

import { fuzzyMatch, highlightRuns } from "./fuzzy";

const score = (query: string, name: string) => {
  const found = fuzzyMatch(query, name);
  if (!found) throw new Error(`${query} should match ${name}`);
  return found.score;
};

describe("fuzzyMatch", () => {
  it("matches everything with an empty query", () => {
    expect(fuzzyMatch("", "Save All")).toEqual({ score: 0, positions: [] });
    expect(fuzzyMatch("  ", "Save All")?.score).toBe(0);
  });

  it("wants the query's characters in order", () => {
    expect(fuzzyMatch("xyz", "Copy Password")).toBeNull();
    expect(fuzzyMatch("ypoc", "Copy Password")).toBeNull();
    expect(fuzzyMatch("copy passwords", "Copy Password")).toBeNull();
  });

  it("ignores case and the query's spaces", () => {
    expect(fuzzyMatch("COPY", "Copy Password")).not.toBeNull();
    expect(fuzzyMatch("copy pass", "Copy Password")).not.toBeNull();
    expect(fuzzyMatch("copypass", "Copy Password")).not.toBeNull();
  });

  it("says where it matched", () => {
    expect(fuzzyMatch("cp", "Copy Password")?.positions).toEqual([0, 5]);
    expect(fuzzyMatch("save", "Save All")?.positions).toEqual([0, 1, 2, 3]);
    expect(fuzzyMatch("ér", "Créer un coffre")?.positions).toEqual([2, 4]);
  });

  it("ranks word starts, runs and short gaps higher", () => {
    expect(score("cp", "Copy Password")).toBeGreaterThan(score("cp", "Accept"));
    expect(score("oe", "Open Entry")).toBeGreaterThan(score("oe", "Shoe"));
    expect(score("user", "Copy Username")).toBeGreaterThan(score("user", "Undo Selected Row"));
    expect(score("cu", "CopyUsername")).toBeGreaterThan(score("cu", "Accumulate"));
    expect(score("ae", "Gate")).toBeGreaterThan(score("ae", "Gaggle"));
  });

  it("picks the best alignment, not the first", () => {
    expect(fuzzyMatch("cpa", "Copy Password")?.positions).toEqual([0, 5, 6]);
  });

  it("matches gh in GitHub", () => {
    expect(fuzzyMatch("gh", "GitHub")?.positions).toEqual([0, 3]);
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
});
