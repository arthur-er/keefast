import { describe, expect, it, vi } from "vitest";

import type { VaultSummary } from "@/bindings";

vi.mock("@/api/queries", () => ({ useWorkspace: () => ({ data: undefined }) }));

const { automaticTagColor, chosenTagColor, tagColor } = await import("./tagColors");

const vault = { tagColors: [{ tag: "dev", color: "green" }] } as unknown as VaultSummary;

describe("tag colours", () => {
  it("picks the same colour for the same name", () => {
    expect(automaticTagColor("dev")).toBe(automaticTagColor("dev"));
    // As the prototype picks them: by the sum of the name's characters (f is 102, a is 97).
    expect(automaticTagColor("f")).toBe("violet");
    expect(automaticTagColor("a")).toBe("blue");
    expect(automaticTagColor("ab")).toBe("orange");
  });

  it("draws a tag in the colour chosen in its Vault, else the one its name picks", () => {
    expect(chosenTagColor(vault, "dev")).toBe("green");
    expect(tagColor(vault, "dev")).toBe("green");
    expect(chosenTagColor(vault, "Dev")).toBeNull();
    expect(tagColor(vault, "Dev")).toBe(automaticTagColor("Dev"));
    expect(tagColor(undefined, "dev")).toBe(automaticTagColor("dev"));
  });
});
