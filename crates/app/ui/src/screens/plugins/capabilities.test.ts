import { describe, expect, it } from "vitest";

import { capabilityLines, listNames } from "./capabilities";

describe("Plugin capabilities in plain words", () => {
  it("lists the Owned fields first, then the rest", () => {
    const lines = capabilityLines({ capabilities: ["copy", "readProtected"], ownedFields: ["IBAN", "BIC"] });
    expect(lines.map((line) => line.kind)).toEqual(["ownedFields", "readProtected", "copy"]);
    expect(lines[0]!.desc).toContain("“IBAN” and “BIC”");
    expect(lines.every((line) => !line.isNew)).toBe(true);
    expect(lines.map((line) => `${line.title} ${line.desc}`).join(" ")).not.toMatch(/permission|grant|scope|extension|add-on/i);
  });

  it("marks what an update asks for anew", () => {
    const lines = capabilityLines(
      { capabilities: ["copy", "readProtected"], ownedFields: ["IBAN", "BIC", "Account holder"] },
      { capabilities: ["readProtected"], ownedFields: ["Account holder"] },
    );
    expect(lines.map((line) => [line.kind, line.isNew])).toEqual([
      ["ownedFields", true],
      ["readProtected", true],
      ["copy", false],
    ]);
  });

  it("joins names as a sentence", () => {
    expect(listNames(["A"])).toBe("“A”");
    expect(listNames(["A", "B", "C"])).toBe("“A”, “B” and “C”");
  });
});
