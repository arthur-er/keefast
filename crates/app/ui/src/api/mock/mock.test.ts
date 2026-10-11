import { describe, expect, it } from "vitest";

import { commands as realCommands } from "@/bindings";

import { ApiError, unwrap } from "../index";
import { createMockBackend } from "./index";
import { giveShortcuts, type PluginManifest } from "./plugins";
import card from "../../../../../../plugins/card/plugin.json";

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

  it("offers to update the Entry a captured login matches, as the backend does", async () => {
    const mock = createMockBackend();
    const api = unwrap(mock.commands);
    mock.simulate("login");
    const login = (await api.getWorkspace()).pendingLogin!;
    expect(login.suggested).toMatchObject({ kind: "update", entry: "w5" });
    expect(login.options[0]!.label).toBe("Update an existing Entry");
    expect(login.options.some((option) => option.target?.kind === "newEntry")).toBe(true);
    await api.saveLogin(login.id, login.suggested);
    expect((await api.getWorkspace()).pendingLogin).toBeNull();
  });

  it("asks for the Entries of the site", async () => {
    const mock = createMockBackend();
    const api = unwrap(mock.commands);
    mock.simulate("access");
    const access = (await api.getWorkspace()).pendingAccess!;
    expect(access.entries.map((entry) => entry.title)).toContain("GitHub");
  });

  it("runs the Card Plugin's component for an Entry's Card, protected values left out", async () => {
    const api = unwrap(createMockBackend().commands);
    const [found] = await api.listEntries({ scope: { kind: "allVaults" }, search: "Ultravioleta", sort: "title" });
    const entry = await api.getEntry(found!.vault, found!.id);
    expect(entry.facets).toHaveLength(1);
    const card = entry.facets[0]!;
    expect(card).toMatchObject({ plugin: "keefast.card", facet: "card", title: "Card", label: "Mastercard •••• 5676", canCopy: true });
    expect(card.fields.map((field) => [field.key, field.value, field.text])).toEqual([
      ["number", null, "Mastercard •••• 5676"],
      ["holder", "ARTHUR REZENDE", null],
      ["expiry", "08/31", null],
      ["cvv", null, null],
      ["pin", null, null],
    ]);
    expect(JSON.stringify(entry.facets)).not.toMatch(/5162|481|8271/);
    const cvv = card.fields[3]!;
    expect(await api.revealCustomField(entry.vault, entry.id, cvv.index!)).toBe("481");

    const github = (await api.listEntries({ scope: { kind: "allVaults" }, search: "github", sort: "title" }))[0]!;
    expect((await api.getEntry(github.vault, github.id)).facets).toEqual([]);
  });

  it("lists the facets Plugins contribute, and runs their format and validate hooks", async () => {
    const api = unwrap(createMockBackend().commands);
    const [card] = await api.listFacets();
    expect(card).toMatchObject({ plugin: "keefast.card", facet: "card", detect: ["Card number"], editor: { add: "Add card", remove: "Remove card" } });
    expect(await api.formatOwnedField("keefast.card", "card", "number", "4111111111111111")).toBe("4111 1111 1111 1111");
    expect(await api.formatOwnedField("keefast.card", "card", "expiry", "083")).toBe("08/3");
    expect(await api.validateOwnedField("keefast.card", "card", "number", "4111 1111 1111 1111")).toBeNull();
    expect(await api.validateOwnedField("keefast.card", "card", "number", "4111 1111 1111 1112")).toMatchObject({ severity: "warning" });
    expect(await api.validateOwnedField("keefast.card", "card", "expiry", "01/20")).toMatchObject({ severity: "warning", message: expect.stringContaining("expired") });
    await expect(api.formatOwnedField("keefast.card", "passport", "number", "1")).rejects.toMatchObject({ kind: "notFound" });
  });
  it("shows a Card's label under the title when the Entry has no username, never a protected value", async () => {
    const api = unwrap(createMockBackend().commands);
    const all = await api.listEntries({ scope: { kind: "allVaults" }, search: "", sort: "title" });
    const shown = (title: string) => {
      const entry = all.find((candidate) => candidate.title === title)!;
      return [entry.username, entry.label];
    };
    expect(shown("Nubank Ultravioleta")).toEqual(["", "Mastercard •••• 5676"]);
    expect(shown("Company Visa")).toEqual(["", "Visa •••• 1111"]);
    expect(shown("GitHub")).toEqual(["arthur-er", null]);
    expect(JSON.stringify(all)).not.toMatch(/5162 9200|4111 1111|"481"|"737"|"8271"/);

    // A username wins over the Card's label.
    const visa = all.find((entry) => entry.title === "Company Visa")!;
    const detail = await api.getEntry(visa.vault, visa.id);
    await api.createEntry(visa.vault, visa.group, {
      title: "Visa with a login", username: "arthur", password: null, url: "", notes: "", tags: [], color: null,
      icon: { kind: "standard", number: 0 }, expires: null,
      customFields: [{ name: "Card number", value: "4111 1111 1111 1111", protected: true, original: null }],
    });
    const [withLogin] = await api.listEntries({ scope: { kind: "allVaults" }, search: "Visa with a login", sort: "title" });
    expect([withLogin!.username, withLogin!.label]).toEqual(["arthur", null]);
    expect(detail.label).toBe("Visa •••• 1111");
  });

  it("gives the Card's copy commands their shortcuts", async () => {
    const api = unwrap(createMockBackend().commands);
    const [found] = await api.listEntries({ scope: { kind: "allVaults" }, search: "Ultravioleta", sort: "title" });
    const [facet] = (await api.getEntry(found!.vault, found!.id)).facets;
    expect(facet!.commands).toEqual([
      { id: "copy-number", title: "Copy card number", field: "number", shortcut: "Ctrl Shift C" },
      { id: "copy-cvv", title: "Copy CVV", field: "cvv", shortcut: "Ctrl Shift V" },
      { id: "copy-expiry", title: "Copy expiry", field: "expiry", shortcut: "Ctrl Shift E" },
    ]);
  });
});

describe("Plugins' shortcuts, as the host gives them", () => {
  const manifest = card as PluginManifest;
  const withShortcuts = (id: string, name: string, shortcuts: string[]): PluginManifest => ({
    ...manifest,
    id,
    name,
    contributions: {
      facets: manifest.contributions.facets.map((facet) => ({
        ...facet,
        commands: (facet.commands ?? []).slice(0, shortcuts.length).map((command, at) => ({ ...command, shortcut: shortcuts[at] })),
      })),
    },
  });

  it("refuses keefast's own, the keys text fields own, and keys that type", () => {
    const { given, refused } = giveShortcuts([withShortcuts("keefast.card", "Card", ["ctrl+shift+n", "Ctrl V", "Shift E"])], ["Ctrl Shift N"]);
    expect(given.size).toBe(0);
    expect(refused.map((r) => [r.command, r.reason])).toEqual([
      ["copy-number", "Ctrl Shift N is one of keefast's own shortcuts"],
      ["copy-cvv", "Ctrl V is one of keefast's own shortcuts"],
      ["copy-expiry", expect.stringContaining("isn't a shortcut a Plugin can have")],
    ]);
  });

  it("refuses another Plugin's, and keeps the rest of the Plugin", () => {
    const { given, refused } = giveShortcuts([manifest, withShortcuts("example.passport", "Passport", ["Ctrl Shift C", "Ctrl J"])], []);
    expect(given.get("keefast.card/card/copy-number")).toBe("Ctrl Shift C");
    expect(given.get("example.passport/card/copy-cvv")).toBe("Ctrl J");
    expect(refused).toEqual([{ plugin: "example.passport", facet: "card", command: "copy-number", reason: "Ctrl Shift C is taken by the Card Plugin" }]);
  });
});
