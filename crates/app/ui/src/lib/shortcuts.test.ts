import { describe, expect, it } from "vitest";

import { comboLabel, matchesCombo, parseCombo } from "./shortcuts";

const key = (key: string, mods: Partial<Record<"ctrlKey" | "shiftKey" | "altKey" | "metaKey", boolean>> = {}) => ({
  key,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
});

describe("shortcuts", () => {
  it("parses combos written as DESIGN.md writes them", () => {
    expect(parseCombo("Ctrl Shift P")).toEqual({ key: "p", ctrl: true, shift: true, alt: false, meta: false });
    expect(parseCombo("ctrl+k")).toEqual({ key: "k", ctrl: true, shift: false, alt: false, meta: false });
  });

  it("matches the palette and its alias, and nothing looser", () => {
    expect(matchesCombo(key("k", { ctrlKey: true }), parseCombo("Ctrl K"))).toBe(true);
    expect(matchesCombo(key("P", { ctrlKey: true, shiftKey: true }), parseCombo("Ctrl Shift P"))).toBe(true);
    expect(matchesCombo(key("p", { ctrlKey: true }), parseCombo("Ctrl Shift P"))).toBe(false);
    expect(matchesCombo(key("k"), parseCombo("Ctrl K"))).toBe(false);
  });

  it("ignores Shift for symbols, which need it on some layouts", () => {
    expect(matchesCombo(key("/", { shiftKey: true }), parseCombo("/"))).toBe(true);
  });

  it("labels combos for key caps", () => {
    expect(comboLabel("Ctrl Shift P")).toEqual(["Ctrl", "Shift", "P"]);
    expect(comboLabel("Escape")).toEqual(["Esc"]);
  });
});
