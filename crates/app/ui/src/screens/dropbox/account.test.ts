import { describe, expect, it } from "vitest";

import type { VaultSummary } from "@/bindings";

import { disconnectConfirmation, dropboxVaults } from "./account";

function vault(name: string, provider: "local" | "dropbox", state: VaultSummary["state"] = "unlocked"): VaultSummary {
  return {
    id: name.length,
    name,
    location: { provider, path: `/Vaults/${name}.kdbx`, name, description: name },
    state,
    unsavedChanges: false,
    saving: false,
    saveFailed: false,
    lastSynced: null,
    syncStatus: provider === "dropbox" ? "synced" : null,
    entryCount: 0,
    rootGroup: null,
    recycleBin: null,
    browserGroup: null,
    smartLists: { withTotp: 0, recentlyModified: 0, expired: 0 },
    tagColors: [],
  };
}

describe("disconnecting Dropbox", () => {
  it("counts the Vaults open from Dropbox, locked ones too", () => {
    const open = [vault("Personal", "dropbox"), vault("Laptop", "local"), vault("Family", "dropbox", "locked")];
    expect(dropboxVaults(open).map((v) => v.name)).toEqual(["Personal", "Family"]);
  });

  it("names the Vaults open from Dropbox", () => {
    expect(disconnectConfirmation([vault("Personal", "dropbox")]).description).toMatch(/^Personal is open from Dropbox\./);
    expect(disconnectConfirmation([vault("Personal", "dropbox"), vault("Family", "dropbox")]).description).toMatch(
      /^Personal and Family are open from Dropbox\./,
    );
  });

  it("says only what disconnecting does when no Vault from Dropbox is open", () => {
    expect(disconnectConfirmation([]).description).not.toMatch(/open from Dropbox/);
  });
});
