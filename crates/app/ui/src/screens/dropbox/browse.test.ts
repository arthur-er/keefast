import { describe, expect, it } from "vitest";

import { ApiError, unwrap } from "@/api";
import { createMockBackend } from "@/api/mock";

import { crumbs, isVaultFile, modifiedLabel, parentFolder, sizeLabel } from "./browse";

describe("browsing Dropbox", () => {
  it("shows the folder as crumbs from the top of the Dropbox", () => {
    expect(crumbs("")).toEqual([{ path: "", name: "Dropbox" }]);
    expect(crumbs("/Apps/keefast")).toEqual([
      { path: "", name: "Dropbox" },
      { path: "/Apps", name: "Apps" },
      { path: "/Apps/keefast", name: "keefast" },
    ]);
  });

  it("goes up a folder until the top", () => {
    expect(parentFolder("/Apps/keefast")).toBe("/Apps");
    expect(parentFolder("/Apps")).toBe("");
    expect(parentFolder("")).toBeNull();
  });

  it("tells Vault files from other files and folders", () => {
    expect(isVaultFile({ name: "Work.KDBX", isFolder: false })).toBe(true);
    expect(isVaultFile({ name: "notes.txt", isFolder: false })).toBe(false);
    expect(isVaultFile({ name: "Vaults.kdbx", isFolder: true })).toBe(false);
  });

  it("says when a file changed as the prototype does", () => {
    const now = new Date(2026, 9, 10, 15, 0).getTime() / 1000;
    expect(modifiedLabel(new Date(2026, 9, 10, 9, 12).getTime() / 1000, now)).toBe("Today, 09:12");
    expect(modifiedLabel(new Date(2026, 9, 9, 21, 40).getTime() / 1000, now)).toBe("Yesterday, 21:40");
    expect(modifiedLabel(new Date(2025, 1, 14, 12).getTime() / 1000, now)).toBe("Feb 14, 2025");
  });

  it("gives sizes in B, whole KB and MB", () => {
    expect(sizeLabel(411)).toBe("411 B");
    expect(sizeLabel(217_088)).toBe("212 KB");
    expect(sizeLabel(1_258_291)).toBe("1.2 MB");
  });
});

describe("the mock's Dropbox", () => {
  it("lists files with their size and time, and folders without", async () => {
    const api = unwrap(createMockBackend().commands);
    const listed = await api.dropboxListFolder("/Apps/keefast");
    expect(listed.find((entry) => entry.name === "Archive")).toMatchObject({ isFolder: true, size: null, modified: null });
    expect(listed.find((entry) => entry.name === "Work.kdbx")).toMatchObject({
      isFolder: false,
      size: 217_088,
      location: { provider: "dropbox", path: "/Apps/keefast/Work.kdbx" },
    });
  });

  it("stops connecting when asked, as cancelled", async () => {
    const api = unwrap(createMockBackend().commands);
    await api.dropboxDisconnect();
    const connecting = api.dropboxConnect().catch((error: unknown) => error);
    await Promise.resolve();
    expect((await api.dropboxStatus()).connecting).toBe(true);
    await api.dropboxCancelConnect();
    const failure = await connecting;
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).kind).toBe("cancelled");
    expect(await api.dropboxStatus()).toMatchObject({ account: null, connecting: false });
  });
});
