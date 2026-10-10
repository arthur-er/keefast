import { describe, expect, it } from "vitest";

import { ApiError, unwrap } from "../index";
import { createMockBackend } from "./index";

describe("the mock's Plugins", () => {
  it("lists keefast's Card as keefast's, and not removable", async () => {
    const api = unwrap(createMockBackend().commands);
    const [card] = (await api.listPlugins()).plugins;
    expect(card).toMatchObject({ id: "keefast.card", bundled: true, enabled: true, publisher: { kind: "keefast" } });
    const refused = await api.removePlugin("keefast.card").catch((error) => error);
    expect(refused).toBeInstanceOf(ApiError);
  });

  it("shows what a new Plugin asks for before installing it", async () => {
    const api = unwrap(createMockBackend().commands);
    const offer = (await api.choosePluginPackage())!;
    expect(offer).toMatchObject({ name: "IBAN", newKey: true, installedVersion: null, publisher: { kind: "unverified", name: "Acme Ltd" } });
    expect(offer.newCapabilities).toEqual(offer.capabilities);
    expect((await api.listPlugins()).plugins.some((plugin) => plugin.id === offer.id)).toBe(false);

    await api.installPlugin(offer.path, offer.digest);
    expect((await api.listPlugins()).plugins.find((plugin) => plugin.id === offer.id)).toMatchObject({ enabled: true, running: true });
  });

  it("says what an update asks for that wasn't agreed to", async () => {
    const mock = createMockBackend();
    mock.simulate("plugin-update");
    const offer = (await unwrap(mock.commands).choosePluginPackage())!;
    expect(offer).toMatchObject({ installedVersion: "1.0.0", version: "2.0.0", newKey: false });
    expect(offer.newCapabilities).toEqual(["readProtected"]);
    expect(offer.newOwnedFields).toEqual(["Account holder"]);
  });

  it("refuses a clash, another key and a tampered package, saying why", async () => {
    for (const scene of ["plugin-clash", "plugin-key-changed", "plugin-malformed"] as const) {
      const mock = createMockBackend();
      mock.simulate(scene);
      const refused = await unwrap(mock.commands).choosePluginPackage().catch((error) => error);
      expect(refused, scene).toBeInstanceOf(ApiError);
      expect(refused.kind).toBe("invalid");
    }
    const mock = createMockBackend();
    mock.simulate("plugin-clash");
    const clash = await unwrap(mock.commands).choosePluginPackage().catch((error) => error);
    expect(clash.message).toContain("“Card number”");
  });

  it("leaves a disabled Plugin's data to the custom fields", async () => {
    const api = unwrap(createMockBackend().commands);
    const [found] = await api.listEntries({ scope: { kind: "allVaults" }, search: "Ultravioleta", sort: "title" });
    await api.setPluginEnabled("keefast.card", false);
    const entry = await api.getEntry(found!.vault, found!.id);
    expect(entry.facets).toEqual([]);
    expect(entry.customFields.map((field) => field.name)).toEqual(["Card number", "Cardholder", "Expiry", "CVV", "PIN"]);
    await api.setPluginEnabled("keefast.card", true);
    expect((await api.getEntry(found!.vault, found!.id)).facets).toHaveLength(1);
  });
});
