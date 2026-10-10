import { describe, expect, it } from "vitest";

import type { EntrySummary } from "@/bindings";

import { isConfident, orderSections, rankByTitle, rankEntries } from "./rank";

const entry = (title: string, username = "", url = ""): EntrySummary => ({ title, username, url }) as EntrySummary;

describe("rankByTitle", () => {
  const commands = [{ title: "Copy username" }, { title: "Copy password" }, { title: "Save all Vaults" }];

  it("puts the best match first and drops the others", () => {
    expect(rankByTitle("cp", commands).map((ranked) => ranked.item.title)).toEqual(["Copy password", "Copy username"]);
    expect(rankByTitle("save", commands).map((ranked) => ranked.item.title)).toEqual(["Save all Vaults"]);
  });

  it("keeps the order of equal matches", () => {
    expect(rankByTitle("copy", commands).map((ranked) => ranked.item.title)).toEqual(["Copy username", "Copy password"]);
  });
});

describe("rankEntries", () => {
  const entries = [entry("GitHub", "arthur-er", "https://github.com"), entry("Google", "arthur.rezende@gmail.com", "https://accounts.google.com"), entry("Stripe", "billing@rezende.dev", "https://dashboard.stripe.com")];

  it("finds Entries by title, highlighting it", () => {
    const [best] = rankEntries("gh", entries);
    expect(best?.item.title).toBe("GitHub");
    expect(best?.positions).toEqual([0, 3]);
  });

  it("finds them by username or site only when sure", () => {
    expect(rankEntries("billing", entries).map((ranked) => ranked.item.title)).toEqual(["Stripe"]);
    expect(rankEntries("dashboard", entries).map((ranked) => ranked.item.title)).toEqual(["Stripe"]);
    // Letters scattered through an address don't count.
    expect(rankEntries("bdv", entries)).toEqual([]);
  });

  it("lists nothing for an empty query", () => {
    expect(rankEntries("  ", entries)).toEqual([]);
  });

  it("is confident when one title match stands out", () => {
    expect(isConfident(rankEntries("gith", entries))).toBe(true);
    expect(isConfident(rankEntries("g", entries))).toBe(false);
    expect(isConfident(rankEntries("billing", entries))).toBe(false);
  });
});

describe("orderSections", () => {
  it("puts the Entry's commands first and unknown sections before App", () => {
    expect(orderSections(["Go to", "Vaults", "Settings", "Entry", "App", "Entry"])).toEqual(["Entry", "Vaults", "Go to", "Settings", "App"]);
  });
});
