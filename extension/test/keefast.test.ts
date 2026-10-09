import { beforeEach, describe, expect, it } from "vitest";
import { KeefastClient, KeefastError, type NativePort, type Pairing } from "../src/keefast";
import { FakeKeefast } from "./fake-keefast";

class MemoryStore {
  pairings: Pairing[] = [];
  async load() {
    return this.pairings;
  }
  async save(pairings: Pairing[]) {
    this.pairings = pairings;
  }
}

let keefast: FakeKeefast;
let store: MemoryStore;
let client: KeefastClient;

beforeEach(() => {
  keefast = new FakeKeefast();
  store = new MemoryStore();
  client = new KeefastClient(keefast.connect, store);
});

const alice = { uuid: "0011", name: "Example", username: "alice", password: "s3cret" };

describe("status", () => {
  it("is not running when the native host can't reach keefast", async () => {
    keefast.running = false;

    expect(await client.status()).toBe("not-running");
  });

  it("is not running when the native host isn't installed", async () => {
    const missing = new KeefastClient(() => disconnectedPort(), store);

    expect(await missing.status()).toBe("not-running");
  });

  it("is locked when keefast has no Vault open", async () => {
    keefast.locked = true;

    expect(await client.status()).toBe("locked");
  });

  it("is unpaired before the user has paired the extension", async () => {
    expect(await client.status()).toBe("unpaired");
  });

  it("is unpaired when the stored Pairing is unknown to the open Vault", async () => {
    store.pairings = [{ id: "Old browser", key: "a2V5" }];

    expect(await client.status()).toBe("unpaired");
  });
});

describe("pair", () => {
  it("pairs with the open Vault under the name the user gives, and remembers it", async () => {
    await client.pair();

    expect(store.pairings).toEqual([{ id: "Chromium", key: keefast.pairings.get("Chromium") }]);
    expect(await client.status()).toBe("paired");
  });

  it("proves it holds the session key, and sends a new identification key", async () => {
    await client.pair();

    const keyExchange = keefast.received.find((m) => m.action === "change-public-keys")!;
    const associate = keefast.received.find((m) => m.action === "associate")!;
    expect(associate.key).toBe(keyExchange.publicKey);
    expect(associate.idKey).not.toBe(keyExchange.publicKey);
  });

  it("fails when the user turns the Pairing down", async () => {
    keefast.pairingName = undefined;

    await expect(client.pair()).rejects.toMatchObject({ reason: "denied" });
    expect(store.pairings).toEqual([]);
  });

  it("keeps Pairings with other Vaults", async () => {
    store.pairings = [{ id: "Work", key: "d29yaw==" }];

    await client.pair();

    expect(store.pairings.map((pairing) => pairing.id)).toEqual(["Work", "Chromium"]);
  });
});

describe("entries", () => {
  beforeEach(async () => {
    keefast.entries.set("example.com", [alice]);
    await client.pair();
  });

  it("gets the Entries for the site from the paired Vaults", async () => {
    const logins = await client.entries("https://example.com/login", "https://example.com/session");

    expect(logins).toEqual([{ ...alice, hasTotp: false, group: "Passwords" }]);
    const request = keefast.received.find((m) => m.action === "get-logins")!;
    expect(request.url).toBe("https://example.com/login");
    expect(request.submitUrl).toBe("https://example.com/session");
    expect(request.keys).toEqual([{ id: "Chromium", key: keefast.pairings.get("Chromium") }]);
  });

  it("has none for a site without Entries", async () => {
    expect(await client.entries("https://elsewhere.org/")).toEqual([]);
  });

  it("waits for the user's Access decision", async () => {
    keefast.holdLogins = true;
    let answered = false;
    const logins = client.entries("https://example.com/").then((found) => {
      answered = true;
      return found;
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(answered).toBe(false);

    keefast.release();

    expect(await logins).toEqual([{ ...alice, hasTotp: false, group: "Passwords" }]);
  });

  it("fails as locked when keefast was locked meanwhile", async () => {
    keefast.locked = true;

    await expect(client.entries("https://example.com/")).rejects.toMatchObject({
      reason: "locked",
    });
  });

  it("rejects a reply that doesn't answer its request's nonce", async () => {
    keefast.wrongNonce = true;

    await expect(client.entries("https://example.com/")).rejects.toBeInstanceOf(KeefastError);
  });

  it("rejects a reply it can't read", async () => {
    keefast.garbled = true;

    await expect(client.entries("https://example.com/")).rejects.toBeInstanceOf(KeefastError);
  });
});

describe("saveLogin", () => {
  const login = {
    url: "https://example.com/login",
    submitUrl: "https://example.com/session",
    username: "alice",
    password: "n3w-s3cret",
  };

  beforeEach(async () => {
    await client.pair();
  });

  it("sends the login for the user to save in keefast", async () => {
    expect(await client.saveLogin({ ...login, uuid: "0011" })).toBe(true);

    expect(keefast.saved).toEqual([
      expect.objectContaining({
        id: "Chromium",
        url: "https://example.com/login",
        submitUrl: "https://example.com/session",
        login: "alice",
        password: "n3w-s3cret",
        uuid: "0011",
      }),
    ]);
  });

  it("proves its Pairing first, as keefast requires", async () => {
    await client.saveLogin(login);

    const actions = keefast.received.map((m) => m.action);
    expect(actions.lastIndexOf("test-associate")).toBeLessThan(actions.indexOf("set-login"));
  });

  it("says so when the user turns the login down", async () => {
    keefast.savesLogins = false;

    expect(await client.saveLogin(login)).toBe(false);
    expect(keefast.saved).toEqual([]);
  });

  it("sends nothing while unpaired", async () => {
    store.pairings = [];

    await expect(client.saveLogin(login)).rejects.toBeInstanceOf(KeefastError);
    expect(keefast.received.map((m) => m.action)).not.toContain("set-login");
  });

  it("fails as locked when keefast is locked", async () => {
    keefast.locked = true;

    await expect(client.saveLogin(login)).rejects.toMatchObject({ reason: "locked" });
  });
});

describe("totp", () => {
  const github = { uuid: "0044", name: "GitHub", username: "alice", password: "s3cret" };

  beforeEach(async () => {
    await client.pair();
  });

  it("says which Entries keefast gave have TOTP", async () => {
    keefast.entries.set("example.com", [alice, { ...github, totp: "123456" }]);

    const entries = await client.entries("https://example.com/");

    expect(entries.map(({ hasTotp }) => hasTotp)).toEqual([false, true]);
  });

  it("gets the current code of an Entry keefast gave for a site", async () => {
    keefast.entries.set("example.com", [{ ...github, totp: "123456" }]);
    await client.entries("https://example.com/");
    keefast.entries.set("example.com", [{ ...github, totp: "654321" }]);
    await client.entries("https://example.com/");

    expect(await client.totp("0044")).toBe("654321");
    expect(keefast.received.at(-1)).toMatchObject({ action: "get-totp", uuid: "0044" });
  });

  it("has none for an Entry keefast didn't give", async () => {
    keefast.entries.set("example.com", [{ ...github, totp: "123456" }]);

    expect(await client.totp("0044")).toBeUndefined();
  });

  it("fails as locked when keefast is locked", async () => {
    keefast.locked = true;

    await expect(client.totp("0044")).rejects.toMatchObject({ reason: "locked" });
  });
});

describe("generatePassword", () => {
  it("gets a password made with the user's generator settings", async () => {
    expect(await client.generatePassword()).toBe("G3nerated!");
  });

  it("fails as denied when the settings can't make a password", async () => {
    keefast.generated = undefined;

    await expect(client.generatePassword()).rejects.toMatchObject({ reason: "denied" });
  });

  it("fails as not running without keefast", async () => {
    keefast.running = false;

    await expect(client.generatePassword()).rejects.toMatchObject({ reason: "not-running" });
  });
});

describe("search", () => {
  beforeEach(async () => {
    keefast.entries.set("other.test", [{ ...alice, group: "Work / Email", totp: "123456" }]);
    await client.pair();
  });

  it("finds Entries in the paired Vaults whatever their site, without passwords", async () => {
    const found = await client.search("https://example.com/login", undefined, "alice");

    expect(found).toEqual([
      {
        uuid: "0011",
        name: "Example",
        username: "alice",
        url: "",
        group: "Work / Email",
        hasTotp: true,
      },
    ]);
    const request = keefast.received.find((m) => m.action === "keefast-search-entries")!;
    expect(request.url).toBe("https://example.com/login");
    expect(request.submitUrl).toBe("https://example.com/login");
    expect(request.query).toBe("alice");
    expect(request.keys).toEqual([{ id: "Chromium", key: keefast.pairings.get("Chromium") }]);
  });

  it("fails as locked when keefast is locked", async () => {
    keefast.locked = true;

    await expect(client.search("https://example.com/", undefined, "a")).rejects.toMatchObject({
      reason: "locked",
    });
  });
});

describe("pick", () => {
  beforeEach(async () => {
    keefast.entries.set("other.test", [{ ...alice, totp: "123456" }]);
    await client.pair();
  });

  it("gets the picked Entry with its password and current TOTP code", async () => {
    const entry = await client.pick("0011", "https://example.com/login", undefined, false);

    expect(entry).toEqual({ ...alice, hasTotp: true, group: "Passwords", totp: "123456" });
  });

  it("asks keefast to add the site to the Entry when told to", async () => {
    await client.pick("0011", "https://example.com/login", undefined, true);

    expect(keefast.picked).toEqual([
      { uuid: "0011", url: "https://example.com/login", addUrl: true },
    ]);
  });

  it("has none for an Entry keefast doesn't give", async () => {
    expect(await client.pick("9999", "https://example.com/", undefined, false)).toBeUndefined();
  });
});

describe("connection", () => {
  it("exchanges keys once per connection", async () => {
    await client.status();
    await client.status();

    expect(keefast.connections).toBe(1);
    expect(keefast.received.filter((m) => m.action === "change-public-keys")).toHaveLength(1);
  });

  it("reconnects, with new keys, once keefast is back", async () => {
    keefast.running = false;
    expect(await client.status()).toBe("not-running");

    keefast.running = true;

    expect(await client.status()).toBe("unpaired");
    expect(keefast.connections).toBe(2);
  });
});

function disconnectedPort(): NativePort {
  const listeners: (() => void)[] = [];
  queueMicrotask(() => listeners.forEach((listener) => listener()));
  return {
    postMessage: () => {},
    onMessage: () => {},
    onDisconnect: (listener) => listeners.push(listener),
    disconnect: () => {},
  };
}
