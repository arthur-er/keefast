import { describe, expect, it } from "vitest";

import { lastUsed, pairedText, usedText } from "./pairings";
import { searchSections, type SearchableSection } from "./search";
import { keySummary } from "./VaultKeySection";
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

describe("a Pairing", () => {
  const now = Date.parse("2026-10-08T12:00:00Z") / 1000;
  const minutes = (n: number) => now - n * 60;

  it("says how long ago its browser last asked, live within the hour", () => {
    expect(usedText(null, now)).toEqual({ text: "Not used yet", live: false });
    expect(usedText(now - 20, now)).toEqual({ text: "Used just now", live: true });
    expect(usedText(minutes(1), now)).toEqual({ text: "Used 1 minute ago", live: true });
    expect(usedText(minutes(2), now)).toEqual({ text: "Used 2 minutes ago", live: true });
    expect(usedText(minutes(60), now)).toEqual({ text: "Used 1 hour ago", live: false });
    expect(usedText(minutes(26 * 60), now).text).toBe("Used yesterday");
    expect(usedText(minutes(3 * 24 * 60), now).text).toBe("Used 3 days ago");
    expect(usedText(minutes(90 * 24 * 60), now).text).toBe("Used 3 months ago");
    expect(usedText(minutes(800 * 24 * 60), now).text).toBe("Used 2 years ago");
  });

  it("says when it was paired, if the Vault knows", () => {
    expect(pairedText(Date.parse("2026-08-02T12:00:00Z") / 1000)).toBe("Paired 2 Aug 2026");
    expect(pairedText(null)).toBeNull();
  });

  it("finds when any was last used", () => {
    const pairing = (lastUsed: number | null) => ({ name: "x", created: null, lastUsed });
    expect(lastUsed([pairing(null), pairing(5), pairing(9), pairing(null)])).toBe(9);
    expect(lastUsed([pairing(null)])).toBeNull();
    expect(lastUsed([])).toBeNull();
  });
});

describe("the Security section", () => {
  it("says what the master key is made of before its encryption", () => {
    const encryption = { cipher: "aes256", kdf: { kind: "argon2id", memoryKib: 65536, iterations: 5, parallelism: 4 } } as const;
    expect(keySummary({ masterKey: "passwordAndKeyFile", encryption })).toBe("Master password + Key file · AES-256 · Argon2id · 64 MiB");
    expect(keySummary({ masterKey: "password", encryption })).toBe("Master password · AES-256 · Argon2id · 64 MiB");
    expect(keySummary({ masterKey: "keyFile", encryption })).toBe("Key file · AES-256 · Argon2id · 64 MiB");
  });
});
