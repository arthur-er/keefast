import { describe, expect, it } from "vitest";

import { commands as realCommands } from "@/bindings";

import { ApiError, unwrap } from "../index";
import { createMockBackend } from "./index";

describe("mock backend", () => {
  it("answers every command the Tauri app has", () => {
    const mock = createMockBackend();
    expect(Object.keys(mock.commands).sort()).toEqual(Object.keys(realCommands).sort());
  });

  it("refuses a wrong master password the way the app does", async () => {
    const api = unwrap(createMockBackend().commands);
    const locked = (await api.getWorkspace()).vaults.find((vault) => vault.state === "locked")!;
    const location = { provider: locked.location.provider, path: locked.location.path };
    const failure = await api.unlockVault(location, { password: "wrong", keyFile: null }).catch((e) => e);
    expect(failure).toBeInstanceOf(ApiError);
    expect(failure.kind).toBe("wrongKey");
    await api.unlockVault(location, { password: "anything", keyFile: null });
    expect((await api.getWorkspace()).vaults.find((vault) => vault.id === locked.id)!.state).toBe("unlocked");
  });

  it("searches and keeps secrets out of Entries until revealed", async () => {
    const api = unwrap(createMockBackend().commands);
    const found = await api.listEntries({ scope: { kind: "allVaults" }, search: "git", sort: "title" });
    expect(found[0]!.title).toBe("GitHub");
    const github = await api.getEntry(found[0]!.vault, found[0]!.id);
    expect(JSON.stringify(github)).not.toContain("x7#Lq9");
    expect(github.customFields[0]!.value).toBeNull();
    expect(await api.revealPassword(github.vault, github.id)).toBe("x7#Lq9!vR2mZ@pW4");
  });

  it("tells the UI about every change with a higher revision", async () => {
    const mock = createMockBackend();
    const api = unwrap(mock.commands);
    const revisions: number[] = [];
    await mock.events.workspaceChanged.listen((event) => revisions.push(event.payload.revision));
    await api.setTheme("dark");
    await api.lockAll();
    expect(revisions.length).toBeGreaterThanOrEqual(2);
    expect(revisions).toEqual([...revisions].sort((a, b) => a - b));
    expect((await api.getSettings()).theme).toBe("dark");
  });

  it("puts browser requests in the snapshot for the dialogs", async () => {
    const mock = createMockBackend();
    const api = unwrap(mock.commands);
    mock.simulate("pairing");
    const pairing = (await api.getWorkspace()).pendingPairing!;
    await expect(api.approvePairing(pairing.id, "  ")).rejects.toThrow("Give this browser a name.");
    await api.approvePairing(pairing.id, "Firefox at home");
    expect((await api.getWorkspace()).pendingPairing).toBeNull();
    expect((await api.getVaultSettings(pairing.vault)).pairings).toContain("Firefox at home");
  });

  it("shows browser integration failing until it is turned on again", async () => {
    const mock = createMockBackend();
    const api = unwrap(mock.commands);
    mock.simulate("browserError");
    expect((await api.getSettings()).browser).toMatchObject({ enabled: true, error: true });
    await api.setBrowserIntegration(true);
    expect((await api.getSettings()).browser).toMatchObject({ error: false, browsers: ["Chrome", "Brave"] });
    mock.simulate("configError");
    expect((await api.getSettings()).configError).toContain("Permission denied");
    expect((await api.getWorkspace()).configError).toContain("Permission denied");
  });
});
