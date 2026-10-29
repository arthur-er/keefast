import { describe, expect, it } from "vitest";

import type { VaultSummary } from "@/bindings";

import { storageLine, toolbarStatus, vaultStatus } from "./vaultStatus";

function vault(changes: Partial<VaultSummary> = {}): VaultSummary {
  return {
    id: 0, name: "Personal", location: { provider: "local", path: "/v/Personal.kdbx", name: "Personal", description: "/v/Personal.kdbx" },
    state: "unlocked", unsavedChanges: false, saving: false, saveFailed: false, syncStatus: null, lastSynced: null, entryCount: 1,
    rootGroup: "r", recycleBin: null, browserGroup: null, ...changes,
  };
}

describe("vaultStatus", () => {
  it("puts saving and failures before sync", () => {
    expect(vaultStatus(vault()).text).toBe("All changes saved");
    expect(vaultStatus(vault({ unsavedChanges: true })).short).toBe("Unsaved");
    expect(vaultStatus(vault({ saveFailed: true, syncStatus: "synced" })).tone).toBe("failed");
    expect(vaultStatus(vault({ syncStatus: "offline" })).text).toBe("Offline");
    expect(vaultStatus(vault({ state: "locked" })).text).toBe("Locked");
  });

  it("says the Storage provider in the switcher", () => {
    expect(storageLine(vault())).toBe("Local · Saved");
    const dropbox = vault({ syncStatus: "synced", location: { provider: "dropbox", path: "/W.kdbx", name: "W", description: "Dropbox: /W.kdbx" } });
    expect(storageLine(dropbox)).toBe("Dropbox · Synced");
  });
});

describe("toolbarStatus", () => {
  it("shows the shown Vault, or what needs attention elsewhere", () => {
    const work = vault({ id: 1, unsavedChanges: true });
    expect(toolbarStatus([vault(), work], vault()).text).toBe("All changes saved");
    expect(toolbarStatus([vault(), work], undefined).text).toBe("Unsaved changes");
    expect(toolbarStatus([vault()], vault({ state: "locked" })).text).toBe("All changes saved");
  });
});
