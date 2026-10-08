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

    expect(logins).toEqual([alice]);
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

    expect(await logins).toEqual([alice]);
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
