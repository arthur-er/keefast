// A fake keefast behind the native host: it answers the KeePassXC-Browser protocol the way
// keefast does, so the client can be tested without a browser or the app.
import nacl from "tweetnacl";
import { decodeBase64, encodeBase64, incrementNonce } from "../src/crypto";
import type { Entry, NativePort } from "../src/keefast";

type Message = Record<string, unknown>;

export class FakeKeefast {
  running = true;
  locked = false;
  /** Pairings as name and identification key. */
  pairings = new Map<string, string>();
  /** Entries by site host. */
  entries = new Map<string, Entry[]>();
  /** The name the user gives the next Pairing, or undefined to deny it. */
  pairingName: string | undefined = "Chromium";
  /** Breaks the nonce of replies, as an impostor would. */
  wrongNonce = false;
  /** Seals replies that aren't JSON. */
  garbled = false;
  /** Holds `get-logins` replies until released, as the Access decision prompt does. */
  holdLogins = false;
  connections = 0;
  received: Message[] = [];
  private held: (() => void)[] = [];

  connect = (): NativePort => {
    this.connections += 1;
    const keys = nacl.box.keyPair();
    let clientKey: Uint8Array | undefined;
    const messageListeners: ((message: object) => void)[] = [];
    const disconnectListeners: (() => void)[] = [];
    let open = true;
    const close = () => {
      if (open) {
        open = false;
        disconnectListeners.forEach((listener) => listener());
      }
    };
    const reply = (message: object) => {
      queueMicrotask(() => open && messageListeners.forEach((listener) => listener(message)));
    };
    const encrypted = (request: Message, fields: Message) => {
      const nonce = incrementNonce(decodeBase64(request.nonce as string));
      if (this.wrongNonce) nonce[0] = (nonce[0]! + 7) % 256;
      const message = { ...fields, success: "true", nonce: encodeBase64(nonce), version: "2.7.10" };
      const sealed = nacl.box(
        new TextEncoder().encode(this.garbled ? "not json" : JSON.stringify(message)),
        nonce,
        clientKey!,
        keys.secretKey,
      );
      return {
        action: request.action,
        message: encodeBase64(sealed),
        nonce: encodeBase64(nonce),
        requestID: request.requestID,
      };
    };
    const error = (request: Message, code: number) => ({
      action: request.action,
      errorCode: String(code),
      error: `error ${code}`,
      requestID: request.requestID,
    });

    const handle = (request: Message) => {
      if (!this.running) {
        reply(error(request, 5));
        queueMicrotask(close);
        return;
      }
      if (request.action === "change-public-keys") {
        clientKey = decodeBase64(request.publicKey as string);
        this.received.push(request);
        const nonce = incrementNonce(decodeBase64(request.nonce as string));
        reply({
          action: "change-public-keys",
          publicKey: encodeBase64(keys.publicKey),
          nonce: encodeBase64(nonce),
          success: "true",
          requestID: request.requestID,
        });
        return;
      }
      const opened = nacl.box.open(
        decodeBase64(request.message as string),
        decodeBase64(request.nonce as string),
        clientKey!,
        keys.secretKey,
      );
      const message = JSON.parse(new TextDecoder().decode(opened!)) as Message;
      this.received.push(message);
      if (this.locked) {
        reply(error(request, 1));
        return;
      }
      switch (message.action) {
        case "get-databasehash":
          reply(encrypted(request, { hash: "vault-hash" }));
          return;
        case "associate": {
          const name = this.pairingName;
          if (name === undefined) {
            reply(error(request, 6));
            return;
          }
          this.pairings.set(name, message.idKey as string);
          reply(encrypted(request, { hash: "vault-hash", id: name }));
          return;
        }
        case "test-associate": {
          const paired = this.pairings.get(message.id as string) === message.key;
          reply(
            paired ? encrypted(request, { hash: "vault-hash", id: message.id }) : error(request, 8),
          );
          return;
        }
        case "get-logins": {
          const keys = message.keys as { id: string; key: string }[];
          const paired = keys.some(({ id, key }) => this.pairings.get(id) === key);
          const host = new URL(message.url as string).host;
          const entries = (paired && this.entries.get(host)) || [];
          const answer = () =>
            reply(
              entries.length === 0
                ? error(request, 15)
                : encrypted(request, {
                    count: entries.length,
                    entries: entries.map(({ uuid, name, username, password }) => ({
                      login: username,
                      name,
                      password,
                      uuid,
                    })),
                    hash: "vault-hash",
                    id: "",
                  }),
            );
          if (this.holdLogins) this.held.push(answer);
          else answer();
          return;
        }
        default:
          reply(error(request, 12));
      }
    };

    return {
      postMessage: (message) => queueMicrotask(() => open && handle(message as Message)),
      onMessage: (listener) => messageListeners.push(listener),
      onDisconnect: (listener) => disconnectListeners.push(listener),
      disconnect: () => {
        open = false;
      },
    };
  };

  /** Answers the held `get-logins` requests. */
  release(): void {
    this.held.splice(0).forEach((answer) => answer());
  }
}
