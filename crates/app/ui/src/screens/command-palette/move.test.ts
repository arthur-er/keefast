import { describe, expect, it } from "vitest";

import type { GroupNode } from "@/bindings";

import { entriesIn, moveTargets } from "./move";

const node = (id: string, entryCount: number, children: GroupNode[] = [], bin = false): GroupNode => ({
  id, name: id, entryCount, totalEntries: entryCount, isRecycleBin: bin, inRecycleBin: bin, isBrowserGroup: false, children,
  icon: { kind: "standard", number: 48 },
});

describe("moveTargets", () => {
  const binned = { ...node("Old", 1), inRecycleBin: true };
  const tree = node("Personal", 2, [node("Email", 3, [node("Work mail", 0)]), node("Finance", 1), node("Recycle Bin", 4, [binned], true)]);

  it("lists the Vault's Groups in tree order with their depth, marking the Entry's own", () => {
    expect(moveTargets(tree, "Email").map(({ group, depth, current }) => [group.id, depth, current])).toEqual([
      ["Personal", 0, false],
      ["Email", 1, true],
      ["Work mail", 2, false],
      ["Finance", 1, false],
    ]);
  });

  it("leaves out the Recycle Bin and what is in it", () => {
    expect(moveTargets(tree, "Personal").map(({ group }) => group.id)).not.toContain("Recycle Bin");
    expect(moveTargets(tree, "Personal").map(({ group }) => group.id)).not.toContain("Old");
  });
});

describe("entriesIn", () => {
  it("counts the Entries right in a Group", () => {
    expect(entriesIn(node("a", 0))).toBe("No Entries");
    expect(entriesIn(node("a", 1))).toBe("1 Entry");
    expect(entriesIn(node("a", 3))).toBe("3 Entries");
  });
});
