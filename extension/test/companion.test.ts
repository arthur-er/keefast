import { beforeEach, describe, expect, it } from "vitest";
import { Companion, type Page, type Tabs } from "../src/companion";
import { KeefastClient, type Entry, type Pairing } from "../src/keefast";
import { FakeKeefast } from "./fake-keefast";

const alice = { uuid: "0011", name: "Example", username: "alice", password: "s3cret" };
const bob = { uuid: "0022", name: "Example (bob)", username: "bob", password: "hunter2" };

class FakeTabs implements Tabs {
  pages = new Map<number, Page>();
  filled: { tab: number; origin: string; entry: Entry }[] = [];
  async inspect(tab: number) {
    return this.pages.get(tab);
  }
  async fill(tab: number, origin: string, entry: Entry) {
    if (this.pages.get(tab)?.origin !== origin) return false;
    this.filled.push({ tab, origin, entry });
    return true;
  }
}

let keefast: FakeKeefast;
let tabs: FakeTabs;
let companion: Companion;

beforeEach(async () => {
  keefast = new FakeKeefast();
  keefast.entries.set("example.com", [alice]);
  tabs = new FakeTabs();
  tabs.pages.set(1, {
    url: "https://example.com/login",
    origin: "https://example.com",
    submitUrl: "https://example.com/session",
  });
  let pairings: Pairing[] = [];
  const client = new KeefastClient(keefast.connect, {
    load: async () => pairings,
    save: async (saved) => {
      pairings = saved;
    },
  });
  await client.pair();
  companion = new Companion(client, tabs);
});

const getLoginsRequests = () => keefast.received.filter((m) => m.action === "get-logins");
const setLoginRequests = () => keefast.received.filter((m) => m.action === "set-login");

describe("from the popup", () => {
  it("lists the page's Entries without their passwords", async () => {
    const logins = await companion.entries(1);

    expect(logins).toEqual([{ uuid: "0011", name: "Example", username: "alice" }]);
    expect(getLoginsRequests()[0]).toMatchObject({
      url: "https://example.com/login",
      submitUrl: "https://example.com/session",
    });
  });

  it("fills the Entry the user picks", async () => {
    await companion.entries(1);

    expect(await companion.fill(1, "0011")).toBe(true);
    expect(tabs.filled).toEqual([{ tab: 1, origin: "https://example.com", entry: alice }]);
  });

  it("doesn't fill a page that navigated to another site after listing", async () => {
    await companion.entries(1);
    tabs.pages.set(1, { url: "https://evil.example/", origin: "https://evil.example" });

    expect(await companion.fill(1, "0011")).toBe(false);
    expect(tabs.filled).toEqual([]);
  });

  it("fills nothing it hasn't listed", async () => {
    expect(await companion.fill(1, "0011")).toBe(false);
    expect(tabs.filled).toEqual([]);
  });

  it("doesn't ask keefast for a page it can't fill", async () => {
    expect(await companion.entries(2)).toBeUndefined();
    expect(getLoginsRequests()).toEqual([]);
  });
});

describe("from the shortcut", () => {
  it("fills the page's only Entry straight away", async () => {
    expect(await companion.shortcut(1)).toBe("filled");
    expect(tabs.filled.map(({ entry }) => entry)).toEqual([alice]);
  });

  it("lets the user choose among several Entries, without asking keefast again", async () => {
    keefast.entries.set("example.com", [alice, bob]);

    expect(await companion.shortcut(1)).toBe("choose");
    expect(await companion.entries(1)).toHaveLength(2);
    expect(getLoginsRequests()).toHaveLength(1);
    expect(tabs.filled).toEqual([]);
  });

  it("shows the popup when there is nothing to fill", async () => {
    keefast.entries.clear();

    expect(await companion.shortcut(1)).toBe("choose");
  });

  it("shows the popup when keefast is locked", async () => {
    keefast.locked = true;

    expect(await companion.shortcut(1)).toBe("choose");
    expect(tabs.filled).toEqual([]);
  });
});

describe("a submitted login", () => {
  const loginPage = {
    url: "https://example.com/login",
    origin: "https://example.com",
    submitUrl: "https://example.com/session",
  };

  it("is offered to keefast to save", async () => {
    await companion.submitted(1, loginPage, { username: "carol", password: "pa55" });

    expect(keefast.saved).toEqual([
      expect.objectContaining({
        url: "https://example.com/login",
        submitUrl: "https://example.com/session",
        login: "carol",
        password: "pa55",
        uuid: "",
      }),
    ]);
  });

  it("isn't offered when it is the Entry just filled", async () => {
    await companion.entries(1);
    await companion.fill(1, "0011");

    await companion.submitted(1, loginPage, { username: "alice", password: "s3cret" });

    expect(setLoginRequests()).toEqual([]);
  });

  it("isn't offered when it is an Entry listed for the site", async () => {
    keefast.entries.set("example.com", [alice, bob]);
    await companion.entries(1);

    await companion.submitted(1, loginPage, { username: "bob", password: "hunter2" });

    expect(setLoginRequests()).toEqual([]);
  });

  it("updates the Entry just filled when the user changed its password", async () => {
    await companion.entries(1);
    await companion.fill(1, "0011");

    await companion.submitted(1, loginPage, { username: "alice", password: "n3w" });

    expect(keefast.saved).toEqual([
      expect.objectContaining({ login: "alice", password: "n3w", uuid: "0011" }),
    ]);
  });

  it("is a new login under another username, even after an Entry was filled", async () => {
    await companion.entries(1);
    await companion.fill(1, "0011");

    await companion.submitted(1, loginPage, { username: "frank", password: "n3w" });

    expect(keefast.saved).toEqual([expect.objectContaining({ login: "frank", uuid: "" })]);
  });

  it("updates the Entry just filled from a password change without a username", async () => {
    await companion.entries(1);
    await companion.fill(1, "0011");

    await companion.submitted(1, loginPage, { username: "", password: "n3w", current: "typo" });

    expect(keefast.saved).toEqual([expect.objectContaining({ login: "alice", uuid: "0011" })]);
  });

  it("updates the listed Entry whose password a password change replaces", async () => {
    keefast.entries.set("example.com", [alice, bob]);
    await companion.entries(1);

    await companion.submitted(1, loginPage, { username: "", password: "n3w", current: "hunter2" });

    expect(keefast.saved).toEqual([
      expect.objectContaining({ login: "bob", password: "n3w", uuid: "0022" }),
    ]);
  });

  it("updates the listed Entry with the same username", async () => {
    keefast.entries.set("example.com", [alice, bob]);
    await companion.entries(1);

    await companion.submitted(1, loginPage, { username: "bob", password: "n3w" });

    expect(keefast.saved).toEqual([expect.objectContaining({ login: "bob", uuid: "0022" })]);
  });

  it("leaves the Entry to keefast for a password change on a page nothing was listed on", async () => {
    await companion.submitted(1, loginPage, { username: "", password: "n3w", current: "old" });

    expect(keefast.saved).toEqual([expect.objectContaining({ login: "", uuid: "" })]);
  });

  it("is offered once, even when submitted again", async () => {
    keefast.savesLogins = false;
    const login = { username: "carol", password: "pa55" };

    await companion.submitted(1, loginPage, login);
    await companion.submitted(1, loginPage, login);

    expect(setLoginRequests()).toHaveLength(1);
  });

  it("is compared with the Entries of the site it was submitted on only", async () => {
    await companion.entries(1);
    await companion.fill(1, "0011");
    const elsewhere = { url: "https://evil.example/login", origin: "https://evil.example" };

    await companion.submitted(1, elsewhere, { username: "alice", password: "s3cret" });

    expect(keefast.saved).toEqual([
      expect.objectContaining({ url: "https://evil.example/login", uuid: "" }),
    ]);
  });

  it("once saved, has the page's Entries asked for again", async () => {
    keefast.entries.clear();
    expect(await companion.entries(1)).toEqual([]);

    await companion.submitted(1, loginPage, { username: "carol", password: "pa55" });
    keefast.entries.set("example.com", [{ uuid: "0033", name: "", username: "carol", password: "pa55" }]);

    expect(await companion.entries(1)).toEqual([{ uuid: "0033", name: "", username: "carol" }]);
  });

  it("can't be saved while keefast is locked", async () => {
    keefast.locked = true;

    await expect(
      companion.submitted(1, loginPage, { username: "carol", password: "pa55" }),
    ).rejects.toMatchObject({ reason: "locked" });
  });
});
