import { beforeEach, describe, expect, it } from "vitest";
import { Companion, type InspectedPage, type Tabs } from "../src/companion";
import type { Credentials, PageFields } from "../src/forms";
import { KeefastClient, type Pairing } from "../src/keefast";
import { FakeKeefast } from "./fake-keefast";

const alice = { uuid: "0011", name: "Example", username: "alice", password: "s3cret" };
const bob = { uuid: "0022", name: "Example (bob)", username: "bob", password: "hunter2" };
const carol = {
  uuid: "0033",
  name: "Example (2FA)",
  username: "carol",
  password: "pa55",
  totp: "123456",
};

const signIn: PageFields = { login: true, totp: false, newPassword: false };
const twoFactor: PageFields = { login: false, totp: true, newPassword: false };
const signUp: PageFields = { login: true, totp: false, newPassword: true };

class FakeTabs implements Tabs {
  pages = new Map<number, InspectedPage>();
  filled: { tab: number; origin: string; credentials: Credentials }[] = [];
  newPasswords: { tab: number; origin: string; password: string }[] = [];
  async inspect(tab: number) {
    return this.pages.get(tab);
  }
  async fill(tab: number, origin: string, credentials: Credentials) {
    if (this.pages.get(tab)?.origin !== origin) return false;
    this.filled.push({ tab, origin, credentials });
    return true;
  }
  async fillNewPassword(tab: number, origin: string, password: string) {
    const page = this.pages.get(tab);
    if (page?.origin !== origin || !page.fields.newPassword) return false;
    this.newPasswords.push({ tab, origin, password });
    return true;
  }
}

/** A page at `url`, with `fields` to fill. */
const at = (url: string, fields: PageFields): InspectedPage => ({
  url,
  origin: new URL(url).origin,
  fields,
});

const withPassword = ({ username, password }: { username: string; password: string }) => ({
  username,
  password,
});

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
    fields: signIn,
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

    expect(logins).toEqual({
      entries: [
        { uuid: "0011", name: "Example", username: "alice", hasTotp: false, group: "Passwords" },
      ],
      fields: signIn,
    });
    expect(getLoginsRequests()[0]).toMatchObject({
      url: "https://example.com/login",
      submitUrl: "https://example.com/session",
    });
  });

  it("fills the Entry the user picks", async () => {
    await companion.entries(1);

    expect(await companion.fill(1, "0011")).toBe(true);
    expect(tabs.filled).toEqual([
      { tab: 1, origin: "https://example.com", credentials: withPassword(alice) },
    ]);
  });

  it("doesn't fill a page that navigated to another site after listing", async () => {
    await companion.entries(1);
    tabs.pages.set(1, at("https://evil.example/", signIn));

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

describe("a 2FA step", () => {
  beforeEach(() => {
    keefast.entries.set("example.com", [alice, carol]);
    tabs.pages.set(1, at("https://example.com/2fa", twoFactor));
  });

  it("lists only the Entries with TOTP", async () => {
    expect(await companion.entries(1)).toEqual({
      entries: [
        {
          uuid: "0033",
          name: "Example (2FA)",
          username: "carol",
          hasTotp: true,
          group: "Passwords",
        },
      ],
      fields: twoFactor,
    });
  });

  it("is filled with the Entry's current TOTP code, asked of keefast", async () => {
    await companion.entries(1);
    keefast.entries.set("example.com", [alice, { ...carol, totp: "654321" }]);

    expect(await companion.fill(1, "0033")).toBe(true);
    expect(tabs.filled.map(({ credentials }) => credentials.totp)).toEqual(["654321"]);
    expect(keefast.received.at(-1)).toMatchObject({ action: "get-totp", uuid: "0033" });
  });

  it("is filled by the shortcut when one Entry has TOTP", async () => {
    expect(await companion.shortcut(1)).toBe("filled");
    expect(tabs.filled.map(({ credentials }) => credentials.totp)).toEqual(["123456"]);
  });

  it("isn't filled when keefast no longer gives the code, and keefast is asked again", async () => {
    await companion.entries(1);
    keefast.entries.set("example.com", [alice]);

    expect(await companion.fill(1, "0033")).toBe(false);
    expect(tabs.filled).toEqual([]);
    await companion.entries(1);
    expect(getLoginsRequests()).toHaveLength(2);
  });

  it("gets no code for a sign-in without a 2FA field", async () => {
    tabs.pages.set(1, at("https://example.com/login", signIn));
    await companion.entries(1);

    await companion.fill(1, "0033");

    expect(tabs.filled.map(({ credentials }) => credentials)).toEqual([withPassword(carol)]);
    expect(keefast.received.map((m) => m.action)).not.toContain("get-totp");
  });

  it("is filled along with the login when the form has both", async () => {
    tabs.pages.set(1, {
      url: "https://example.com/login",
      origin: "https://example.com",
      fields: { login: true, totp: true, newPassword: false },
    });
    await companion.entries(1);

    await companion.fill(1, "0033");

    expect(tabs.filled.map(({ credentials }) => credentials)).toEqual([
      { ...withPassword(carol), totp: "123456" },
    ]);
  });
});

describe("the generator", () => {
  beforeEach(() => {
    tabs.pages.set(1, at("https://example.com/join", signUp));
  });

  it("makes a password with keefast's generator", async () => {
    expect(await companion.generate(1)).toBe("G3nerated!");
    expect(keefast.received.at(-1)).toMatchObject({ action: "generate-password" });
  });

  it("fills the password it made into the page's new password fields", async () => {
    await companion.generate(1);

    expect(await companion.fillGenerated(1)).toBe(true);
    expect(tabs.newPasswords).toEqual([
      { tab: 1, origin: "https://example.com", password: "G3nerated!" },
    ]);
  });

  it("doesn't fill a page that navigated to another site meanwhile", async () => {
    await companion.generate(1);
    tabs.pages.set(1, at("https://evil.example/", signUp));

    expect(await companion.fillGenerated(1)).toBe(false);
    expect(tabs.newPasswords).toEqual([]);
  });

  it("fills a password it made once", async () => {
    await companion.generate(1);
    await companion.fillGenerated(1);

    expect(await companion.fillGenerated(1)).toBe(false);
    expect(tabs.newPasswords).toHaveLength(1);
  });

  it("makes nothing for a page it can't fill", async () => {
    expect(await companion.generate(2)).toBeUndefined();
    expect(keefast.received.map((m) => m.action)).not.toContain("generate-password");
  });
});

describe("from the shortcut", () => {
  it("fills the page's only Entry straight away", async () => {
    expect(await companion.shortcut(1)).toBe("filled");
    expect(tabs.filled.map(({ credentials }) => credentials)).toEqual([withPassword(alice)]);
  });

  it("lets the user choose among several Entries, without asking keefast again", async () => {
    keefast.entries.set("example.com", [alice, bob]);

    expect(await companion.shortcut(1)).toBe("choose");
    expect((await companion.entries(1))?.entries).toHaveLength(2);
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

describe("from the palette's search", () => {
  beforeEach(() => {
    keefast.entries.set("other.test", [bob, carol]);
  });

  it("finds Entries in the paired Vaults for the page", async () => {
    const found = await companion.search(1, "bob");

    expect(found?.map(({ uuid }) => uuid)).toEqual(["0022"]);
    const request = keefast.received.find((m) => m.action === "keefast-search-entries")!;
    expect(request.url).toBe("https://example.com/login");
    expect(request.submitUrl).toBe("https://example.com/session");
  });

  it("doesn't ask keefast for nothing typed", async () => {
    expect(await companion.search(1, "  ")).toEqual([]);
    expect(keefast.received.some((m) => m.action === "keefast-search-entries")).toBe(false);
  });

  it("finds nothing for a page it can't fill", async () => {
    expect(await companion.search(2, "bob")).toBeUndefined();
  });

  it("fills the Entry the user picks, and tells keefast whether to add the site", async () => {
    expect(await companion.fillPicked(1, "0022", true)).toBe(true);

    expect(tabs.filled).toEqual([
      { tab: 1, origin: "https://example.com", credentials: withPassword(bob) },
    ]);
    expect(keefast.picked).toEqual([
      { uuid: "0022", url: "https://example.com/login", addUrl: true },
    ]);
  });

  it("fills a 2FA step with the code that came with the picked Entry", async () => {
    tabs.pages.set(1, at("https://example.com/2fa", twoFactor));

    expect(await companion.fillPicked(1, "0033", false)).toBe(true);

    expect(tabs.filled[0]?.credentials.totp).toBe("123456");
    expect(keefast.received.some((m) => m.action === "get-totp")).toBe(false);
  });

  it("doesn't fill a 2FA step with an Entry without TOTP", async () => {
    tabs.pages.set(1, at("https://example.com/2fa", twoFactor));

    expect(await companion.fillPicked(1, "0022", false)).toBe(false);
    expect(tabs.filled).toEqual([]);
  });

  it("fills nothing keefast doesn't give", async () => {
    expect(await companion.fillPicked(1, "9999", false)).toBe(false);
    expect(tabs.filled).toEqual([]);
  });

  it("asks keefast for the site's Entries again afterwards", async () => {
    await companion.entries(1);
    await companion.fillPicked(1, "0022", true);

    await companion.entries(1);

    expect(getLoginsRequests()).toHaveLength(2);
  });

  it("doesn't offer the picked Entry to save once signed in with it", async () => {
    await companion.fillPicked(1, "0022", false);

    await companion.submitted(
      1,
      { url: "https://example.com/login", origin: "https://example.com" },
      { username: "bob", password: "hunter2" },
    );

    expect(setLoginRequests()).toEqual([]);
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

  it("updates the site's only Entry from a password change it can't place otherwise", async () => {
    await companion.entries(1);

    await companion.submitted(1, loginPage, { username: "", password: "n3w", current: "typo" });

    expect(keefast.saved).toEqual([expect.objectContaining({ login: "alice", uuid: "0011" })]);
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
    expect((await companion.entries(1))?.entries).toEqual([]);

    await companion.submitted(1, loginPage, { username: "carol", password: "pa55" });
    keefast.entries.set("example.com", [
      { uuid: "0033", name: "", username: "carol", password: "pa55" },
    ]);

    expect((await companion.entries(1))?.entries).toEqual([
      { uuid: "0033", name: "", username: "carol", hasTotp: false, group: "Passwords" },
    ]);
  });

  it("can't be saved while keefast is locked", async () => {
    keefast.locked = true;

    await expect(
      companion.submitted(1, loginPage, { username: "carol", password: "pa55" }),
    ).rejects.toMatchObject({ reason: "locked" });
  });
});
