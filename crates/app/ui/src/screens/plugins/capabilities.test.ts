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

  it("says which fields browser fill puts into which forms", () => {
    const lines = capabilityLines({
      capabilities: ["readProtected", "copy", "browserFill"],
      ownedFields: ["Card number", "Cardholder", "Expiry", "CVV", "PIN"],
      filledFields: ["Card number", "Cardholder", "Expiry", "CVV"],
      fillForms: ["payment"],
    });
    const fill = lines.find((line) => line.kind === "browserFill")!;
    expect(lines.at(-1)).toBe(fill);
    expect(fill.title).toBe("Fill payment forms in your browser");
    expect(fill.desc).toContain("“Card number”, “Cardholder”, “Expiry” and “CVV”");
    expect(fill.desc).not.toContain("PIN");
    expect(`${fill.title} ${fill.desc}`).not.toMatch(/permission|grant|scope|extension|add-on/i);
  });

  it("asks again when an update fills one more field", () => {
    const plugin = {
      capabilities: ["browserFill"] as const,
      ownedFields: ["Gift card", "Gift code"],
      filledFields: ["Gift card", "Gift code"],
      fillForms: ["payment"] as const,
    };
    const [, fill] = capabilityLines(plugin, { capabilities: [], ownedFields: [], filledFields: ["Gift code"] });
    expect(fill!.isNew).toBe(true);
    const [, same] = capabilityLines(plugin, { capabilities: [], ownedFields: [], filledFields: [] });
    expect(same!.isNew).toBe(false);
  });

  it("joins names as a sentence", () => {
    expect(listNames(["A"])).toBe("“A”");
    expect(listNames(["A", "B", "C"])).toBe("“A”, “B” and “C”");
  });
});
