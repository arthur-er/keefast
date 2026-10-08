// Talking to the running keefast app through its native host, in the KeePassXC-Browser
// protocol (ADR 0003). The extension holds no secrets of its own (ADR 0001): it never asks for
// a master password, and only asks keefast for an Entry when the user wants to fill it.
import nacl from "tweetnacl";
import { decodeBase64, encodeBase64, incrementNonce, randomNonce } from "./crypto";

/** The native-messaging host keefast registers, under KeePassXC-Browser's name. */
export const HOST_NAME = "org.keepassxc.keepassxc_browser";

/** A connection to the native host, as `chrome.runtime.connectNative` gives one. */
export interface NativePort {
  postMessage(message: object): void;
  onMessage(listener: (message: object) => void): void;
  /** Called once the host has gone: it exited, or isn't installed. */
  onDisconnect(listener: () => void): void;
  disconnect(): void;
}

/** A Pairing with one Vault, as the extension remembers it. */
export interface Pairing {
  /** The name the user gave the Pairing in keefast. */
  id: string;
  /** The identification public key the extension proves itself with, Base64-encoded. */
  key: string;
}

/** Where the extension remembers its Pairings. */
export interface PairingStore {
  load(): Promise<Pairing[]>;
  save(pairings: Pairing[]): Promise<void>;
}

/** An Entry keefast gave for a site. */
export interface Entry {
  uuid: string;
  /** The Entry's title. */
  name: string;
  username: string;
  password: string;
}

/** A login the user submitted, for keefast to save. */
export interface CapturedLogin {
  /** The page it was submitted on. */
  url: string;
  /** Where its form submitted to. */
  submitUrl?: string;
  username: string;
  password: string;
  /** The Entry it updates, when the extension knows which. */
  uuid?: string;
}

/** Whether the extension can fill sign-in forms, and what stands in the way. */
export type Status = "not-running" | "locked" | "unpaired" | "paired";

export type FailureReason = "not-running" | "locked" | "denied" | "failed";

export class KeefastError extends Error {
  constructor(
    readonly reason: FailureReason,
    message: string,
    /** The protocol's error code, when keefast answered with one. */
    readonly code?: string,
  ) {
    super(message);
    this.name = "KeefastError";
  }
}

const notRunning = () => new KeefastError("not-running", "keefast is not running");
const unanswered = () => new KeefastError("failed", "keefast's reply doesn't answer the request");

/** How long keefast has to answer a request that doesn't wait for the user. */
const ANSWER_TIMEOUT_MS = 10_000;

// The protocol's error codes, as KeePassXC numbers them.
const DATABASE_NOT_OPENED = "1";
const NOT_RUNNING = "5";
const ACTION_CANCELLED_OR_DENIED = "6";
const NO_LOGINS_FOUND = "15";

type Message = Record<string, unknown>;

interface Pending {
  resolve(reply: Message): void;
  reject(error: KeefastError): void;
}

/** One connection to the native host, with its session keys once exchanged. */
interface Session {
  port: NativePort;
  keys: nacl.BoxKeyPair;
  hostKey?: Uint8Array;
  pending: Map<string, Pending>;
}

export class KeefastClient {
  private session?: Session;
  /** The key exchange of the current session, which every request waits for. */
  private ready?: Promise<Session>;
  private readonly clientId = encodeBase64(nacl.randomBytes(24));
  private nextRequest = 0;

  constructor(
    private readonly connect: () => NativePort,
    private readonly store: PairingStore,
  ) {}

  /** Whether keefast is running, unlocked, and paired with the extension. */
  async status(): Promise<Status> {
    try {
      await this.checkUnlocked();
      return (await this.provenPairing()) ? "paired" : "unpaired";
    } catch (error) {
      if (error instanceof KeefastError && error.reason !== "failed" && error.reason !== "denied") {
        return error.reason;
      }
      throw error;
    }
  }

  /**
   * Asks keefast to pair the extension with the Vault the user is looking at. keefast asks the
   * user to approve it and name it, so this waits for them.
   */
  async pair(): Promise<Pairing> {
    const idKey = encodeBase64(nacl.box.keyPair().publicKey);
    // The Pairing goes to the Vault keefast deals with the browser in, which this picks.
    await this.checkUnlocked();
    const session = await this.keyExchange();
    const reply = await this.sendEncrypted(
      "associate",
      { key: encodeBase64(session.keys.publicKey), idKey },
      { waitsForUser: true },
    );
    const pairing = { id: String(reply.id), key: idKey };
    const others = (await this.store.load()).filter((other) => other.id !== pairing.id);
    await this.store.save([...others, pairing]);
    return pairing;
  }

  /**
   * The Entries for the page at `url`, whose form submits to `submitUrl`, from the Vaults the
   * extension is paired with. keefast may ask the user first, so this waits for them.
   */
  async entries(url: string, submitUrl?: string): Promise<Entry[]> {
    const pairings = await this.store.load();
    try {
      const reply = await this.sendEncrypted(
        "get-logins",
        {
          url,
          submitUrl: submitUrl ?? url,
          keys: pairings.map(({ id, key }) => ({ id, key })),
        },
        { waitsForUser: true },
      );
      return Array.isArray(reply.entries) ? reply.entries.map(toEntry) : [];
    } catch (error) {
      if (error instanceof KeefastError && error.code === NO_LOGINS_FOUND) return [];
      throw error;
    }
  }

  /**
   * Sends a login the user submitted to keefast, which asks the user where it goes, so this
   * waits for them. Says whether they saved it. keefast takes logins only from an extension
   * that proved a Pairing on this connection, so this proves one first.
   */
  async saveLogin({ url, submitUrl, username, password, uuid }: CapturedLogin): Promise<boolean> {
    const pairing = await this.provenPairing();
    if (!pairing) {
      throw new KeefastError("failed", "The extension isn't paired with an open Vault");
    }
    try {
      await this.sendEncrypted(
        "set-login",
        {
          url,
          submitUrl: submitUrl ?? url,
          id: pairing.id,
          login: username,
          password,
          uuid: uuid ?? "",
          // keefast lets the user choose the Group.
          group: "",
          groupUuid: "",
        },
        { waitsForUser: true },
      );
      return true;
    } catch (error) {
      if (error instanceof KeefastError && error.reason === "denied") return false;
      throw error;
    }
  }

  /** Fails unless keefast is running with a Vault open. */
  private async checkUnlocked(): Promise<void> {
    const pairings = await this.store.load();
    await this.sendEncrypted("get-databasehash", {
      connectedKeys: pairings.map(({ id }) => id),
    });
  }

  /** The first stored Pairing keefast knows, proven on this connection. */
  private async provenPairing(): Promise<Pairing | undefined> {
    for (const pairing of await this.store.load()) {
      if (await this.isPaired(pairing)) return pairing;
    }
    return undefined;
  }

  private async isPaired(pairing: Pairing): Promise<boolean> {
    try {
      await this.sendEncrypted("test-associate", { id: pairing.id, key: pairing.key });
      return true;
    } catch (error) {
      if (error instanceof KeefastError && error.reason === "failed") return false;
      throw error;
    }
  }

  /** Sends an encrypted request, and opens its reply. */
  private async sendEncrypted(
    action: string,
    fields: Message,
    { waitsForUser = false } = {},
  ): Promise<Message> {
    const session = await this.keyExchange();
    const nonce = randomNonce();
    const sealed = nacl.box(
      new TextEncoder().encode(JSON.stringify({ action, ...fields })),
      nonce,
      session.hostKey!,
      session.keys.secretKey,
    );
    const reply = await this.request(
      session,
      { action, message: encodeBase64(sealed), nonce: encodeBase64(nonce) },
      waitsForUser,
    );
    const expected = encodeBase64(incrementNonce(nonce));
    if (reply.nonce !== expected || typeof reply.message !== "string") {
      throw unanswered();
    }
    const opened = nacl.box.open(
      decodeBase64(reply.message),
      decodeBase64(expected),
      session.hostKey!,
      session.keys.secretKey,
    );
    if (!opened) {
      throw new KeefastError("failed", "keefast's reply can't be opened");
    }
    const message = parseMessage(opened);
    if (message?.nonce !== expected) {
      throw unanswered();
    }
    return message;
  }

  /** The current session, connecting and exchanging keys first if there is none. */
  private keyExchange(): Promise<Session> {
    if (!this.ready) {
      const session = this.open();
      const nonce = randomNonce();
      this.ready = this.request(session, {
        action: "change-public-keys",
        publicKey: encodeBase64(session.keys.publicKey),
        nonce: encodeBase64(nonce),
      }).then((reply) => {
        if (
          reply.nonce !== encodeBase64(incrementNonce(nonce)) ||
          typeof reply.publicKey !== "string"
        ) {
          throw new KeefastError("failed", "keefast's key exchange failed");
        }
        session.hostKey = decodeBase64(reply.publicKey);
        return session;
      });
      // A failed exchange leaves no session behind: the next request starts over.
      this.ready.catch(() => {
        if (this.session === session) this.close(session);
      });
    }
    return this.ready;
  }

  private open(): Session {
    const session: Session = {
      port: this.connect(),
      keys: nacl.box.keyPair(),
      pending: new Map(),
    };
    this.session = session;
    session.port.onMessage((reply) => this.receive(session, reply as Message));
    session.port.onDisconnect(() => this.close(session));
    return session;
  }

  /** Ends a session, failing its requests still waiting, so the next request reconnects. */
  private close(session: Session, error?: KeefastError): void {
    if (this.session === session) {
      this.session = undefined;
      this.ready = undefined;
      session.port.disconnect();
    }
    const pending = [...session.pending.values()];
    session.pending.clear();
    const reason = error ?? notRunning();
    pending.forEach(({ reject }) => reject(reason));
  }

  private request(session: Session, message: Message, waitsForUser = false): Promise<Message> {
    const requestID = String(this.nextRequest++);
    return new Promise((resolve, reject) => {
      const timer = waitsForUser
        ? undefined
        : setTimeout(() => {
            this.close(session, new KeefastError("not-running", "keefast didn't answer"));
          }, ANSWER_TIMEOUT_MS);
      session.pending.set(requestID, {
        resolve: (reply) => {
          clearTimeout(timer);
          resolve(reply);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      session.port.postMessage({ ...message, clientID: this.clientId, requestID });
    });
  }

  private receive(session: Session, reply: Message): void {
    const requestID = String(reply.requestID);
    const pending = session.pending.get(requestID);
    if (!pending) return;
    session.pending.delete(requestID);
    if (reply.errorCode === undefined) {
      pending.resolve(reply);
    } else {
      pending.reject(protocolError(String(reply.errorCode)));
    }
  }
}

/** A decrypted message, or none if it isn't a JSON object. */
function parseMessage(bytes: Uint8Array): Message | undefined {
  try {
    const message: unknown = JSON.parse(new TextDecoder().decode(bytes));
    return typeof message === "object" && message !== null ? (message as Message) : undefined;
  } catch {
    return undefined;
  }
}

function protocolError(code: string): KeefastError {
  switch (code) {
    case DATABASE_NOT_OPENED:
      return new KeefastError("locked", "keefast is locked", code);
    case NOT_RUNNING:
      return new KeefastError("not-running", "keefast is not running", code);
    case ACTION_CANCELLED_OR_DENIED:
      return new KeefastError("denied", "Turned down in keefast", code);
    default:
      return new KeefastError("failed", `keefast answered with error ${code}`, code);
  }
}

function toEntry(entry: Record<string, unknown>): Entry {
  return {
    uuid: String(entry.uuid ?? ""),
    name: String(entry.name ?? ""),
    username: String(entry.login ?? ""),
    password: String(entry.password ?? ""),
  };
}
