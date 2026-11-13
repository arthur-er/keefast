import { describe, expect, it } from "vitest";

import { onSmartList, RECENTLY_MODIFIED_SECONDS, smartListName } from "./smartLists";

const NOW = 1_800_000_000;
const entry = { hasTotp: false, modified: null as number | null, expired: false };

describe("Smart lists", () => {
  it("With TOTP has the Entries with TOTP set up", () => {
    expect(onSmartList("withTotp", { ...entry, hasTotp: true }, NOW)).toBe(true);
    expect(onSmartList("withTotp", entry, NOW)).toBe(false);
  });

  it("Recently modified has the Entries changed in the last 30 days", () => {
    const changed = (ago: number) => onSmartList("recentlyModified", { ...entry, modified: NOW - ago }, NOW);
    expect(changed(0)).toBe(true);
    expect(changed(RECENTLY_MODIFIED_SECONDS)).toBe(true);
    expect(changed(RECENTLY_MODIFIED_SECONDS + 1)).toBe(false);
    expect(onSmartList("recentlyModified", entry, NOW)).toBe(false);
  });

  it("Expired has the Entries the backend says expired", () => {
    expect(onSmartList("expired", { ...entry, expired: true }, NOW)).toBe(true);
    expect(onSmartList("expired", entry, NOW)).toBe(false);
  });

  it("each is named as the sidebar shows it", () => {
    expect(smartListName("withTotp")).toBe("With TOTP");
    expect(smartListName("recentlyModified")).toBe("Recently modified");
    expect(smartListName("expired")).toBe("Expired");
  });
});
