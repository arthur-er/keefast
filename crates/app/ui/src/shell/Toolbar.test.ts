import { describe, expect, it } from "vitest";

import { mergedWhen, splitMessage } from "./Toolbar";

describe("the Merge notice", () => {
  it("leads with what was merged into where", () => {
    expect(splitMessage("Merged changes made elsewhere into Work: 2 Entries added")).toEqual([
      "Merged changes made elsewhere into Work",
      "2 Entries added",
    ]);
    expect(splitMessage("Merged")).toEqual(["Merged", ""]);
  });

  it("says when", () => {
    expect(mergedWhen(0, 30_000)).toBe("Just now");
    expect(mergedWhen(0, 5 * 60_000)).toBe("5 min ago");
  });
});
