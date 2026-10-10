import { describe, expect, it } from "vitest";

import type { EntrySummary } from "@/bindings";

import { entryKey, groupEntries, moveSelection, visibleEntries } from "./grouping";

function entry(vault: number, id: string, group: string, groupName: string, vaultName = vault === 0 ? "Personal" : "Work"): EntrySummary {
  return {
    vault, id, title: id, username: "", url: "", tags: [], color: null, icon: { kind: "standard", number: 0 },
    faviconUrl: null, expires: null, expired: false, created: null, modified: null, hasPassword: true, hasTotp: false,
    attachmentCount: 0, group, groupName, vaultName, inRecycleBin: false,
  };
}

const topGroups = new Map([
  [0, "root"],
  [1, "wroot"],
]);

const list = [
  entry(0, "amazon", "root", "Personal"),
  entry(0, "figma", "browser", "Browser logins"),
  entry(0, "fastmail", "email", "Email"),
  entry(0, "github", "root", "Personal"),
  entry(0, "google", "email", "Email"),
];

describe("groupEntries", () => {
  it("heads the list with the Vault's own Entries, then its Groups by name", () => {
    const groups = groupEntries(list, { topGroups, acrossVaults: false, searching: false });
    expect(groups.map((g) => [g.label, g.entries.map((e) => e.id)])).toEqual([
      ["Personal", ["amazon", "github"]],
      ["Browser logins", ["figma"]],
      ["Email", ["fastmail", "google"]],
    ]);
    expect(groups[0]!.top).toBe(true);
  });

  it("keeps the best match first when searching", () => {
    const ranked = [list[2]!, list[0]!, list[4]!];
    const groups = groupEntries(ranked, { topGroups, acrossVaults: false, searching: true });
    expect(groups.map((g) => g.label)).toEqual(["Email", "Personal"]);
    expect(groups[0]!.entries.map((e) => e.id)).toEqual(["fastmail", "google"]);
  });

  it("says the Vault in headings when the list spans Vaults", () => {
    const groups = groupEntries([entry(1, "aws", "infra", "Infrastructure"), entry(0, "github", "root", "Personal")], {
      topGroups,
      acrossVaults: true,
      searching: false,
    });
    expect(groups.map((g) => g.label)).toEqual(["Personal", "Work / Infrastructure"]);
  });
});

describe("keyboard moves", () => {
  const groups = groupEntries(list, { topGroups, acrossVaults: false, searching: false });
  const key = (e: EntrySummary | null) => (e ? entryKey(e) : null);
  const at = (id: string) => ({ vault: 0, entry: id });

  it("follow the order shown, across Groups", () => {
    const shown = visibleEntries(groups, new Set());
    expect(shown.map((e) => e.id)).toEqual(["amazon", "github", "figma", "fastmail", "google"]);
    expect(key(moveSelection(shown, at("github"), "next"))).toBe("0:figma");
    expect(key(moveSelection(shown, at("figma"), "previous"))).toBe("0:github");
  });

  it("stop at the ends, and start from them with nothing selected", () => {
    const shown = visibleEntries(groups, new Set());
    expect(key(moveSelection(shown, at("google"), "next"))).toBe("0:google");
    expect(key(moveSelection(shown, at("amazon"), "previous"))).toBe("0:amazon");
    expect(key(moveSelection(shown, null, "next"))).toBe("0:amazon");
    expect(key(moveSelection(shown, null, "previous"))).toBe("0:google");
    expect(key(moveSelection(shown, at("figma"), "first"))).toBe("0:amazon");
    expect(key(moveSelection(shown, at("figma"), "last"))).toBe("0:google");
    expect(moveSelection([], null, "next")).toBeNull();
  });

  it("skip collapsed Groups", () => {
    const shown = visibleEntries(groups, new Set(["0:browser"]));
    expect(key(moveSelection(shown, at("github"), "next"))).toBe("0:fastmail");
  });
});
