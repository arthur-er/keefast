import { afterEach, describe, expect, it } from "vitest";

import { comboFromEvent, comboLabel, DEFAULT_SHORTCUTS, holderOf, matchesCombo, parseCombo, setChangedShortcuts, SHORTCUTS } from "./shortcuts";

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

describe("shortcuts the user changes", () => {
  afterEach(() => setChangedShortcuts({}));

  it("are the keys in effect, the others keeping their defaults", () => {
    expect(SHORTCUTS.search).toEqual(DEFAULT_SHORTCUTS.search);
    setChangedShortcuts({ search: ["Ctrl J"] });
    expect(SHORTCUTS.search).toEqual(["Ctrl J"]);
    expect(SHORTCUTS.palette).toEqual(DEFAULT_SHORTCUTS.palette);
    expect(Object.values(SHORTCUTS).flat()).toContain("Ctrl J");
    setChangedShortcuts({});
    expect(SHORTCUTS.search).toEqual(DEFAULT_SHORTCUTS.search);
  });

  it("are recorded from the key pressed with its modifiers", () => {
    expect(comboFromEvent(key("j", { ctrlKey: true }))).toBe("Ctrl J");
    expect(comboFromEvent(key("K", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl Shift K");
    expect(comboFromEvent(key("ArrowDown", { altKey: true }))).toBe("Alt ArrowDown");
    // Shift makes the symbol, and matching ignores it.
    expect(comboFromEvent(key("?", { shiftKey: true }))).toBe("?");
    expect(comboFromEvent(key("Control", { ctrlKey: true }))).toBeNull();
    expect(comboFromEvent(key(" "))).toBe("Space");
  });

  it("say which other shortcut has a key, as keys match", () => {
    const holders = [{ title: "Command palette", keys: ["Ctrl K", "Ctrl Shift P"] }, { title: "Search", keys: ["Ctrl F", "/"] }];
    expect(holderOf("ctrl+k", holders)?.title).toBe("Command palette");
    expect(holderOf("Ctrl Shift P", holders)?.title).toBe("Command palette");
    expect(holderOf("/", holders)?.title).toBe("Search");
    expect(holderOf("Ctrl P", holders)).toBeUndefined();
  });
});
