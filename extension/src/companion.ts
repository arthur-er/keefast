// What the popup and the shortcut do: ask keefast for the page's Entries, and fill the one the
// user picks. Passwords stay here, never in the popup, and only until filled. Then, what the
// page does: offer keefast the logins the user submits there that it doesn't have yet.
import nacl from "tweetnacl";
import type { SubmittedLogin } from "./capture";
import { encodeBase64 } from "./crypto";
import { KeefastError, type Entry, type KeefastClient } from "./keefast";

/** A page the extension may fill, as its login form sees it. */
export interface Page {
  url: string;
  origin: string;
  /** Where the page's login form submits to. */
  submitUrl?: string;
}

/** The browser's tabs, as far as filling them goes. */
export interface Tabs {
  /** The page in a tab, or none if the extension can't fill it. */
  inspect(tab: number): Promise<Page | undefined>;
  /** Fills an Entry into the tab's login form, if the tab is still at `origin`. */
  fill(tab: number, origin: string, entry: Entry): Promise<boolean>;
}

/** An Entry as the popup lists it, without its password. */
export interface EntrySummary {
  uuid: string;
  name: string;
  username: string;
}

/** How long the Entries keefast gave for a tab are kept for the user to pick one. */
const KEEP_ENTRIES_MS = 2 * 60_000;

/**
 * How long the extension remembers which Entries a tab's site has, to tell a login submitted
 * there from one keefast already has.
 */
const KNOW_ENTRIES_MS = 15 * 60_000;

interface Listed {
  origin: string;
  entries: Entry[];
  expiry: ReturnType<typeof setTimeout>;
}

/** An Entry the extension listed for a site, without its password. */
interface KnownEntry {
  uuid: string;
  username: string;
  /** The Entry's username and password, as {@link Companion.fingerprint} hashes them. */
  login: string;
  /** Its password alone, hashed likewise, to match the password a password change replaces. */
  password: string;
}

/** What the extension knows of the site in a tab, to make sense of logins submitted there. */
interface Known {
  origin: string;
  entries: KnownEntry[];
  /** The Entry last filled in the tab. */
  filled?: string;
  /** The logins already offered to keefast from the tab, hashed. */
  offered: Set<string>;
  expiry?: ReturnType<typeof setTimeout>;
}

export class Companion {
  private listed = new Map<number, Listed>();
  private known = new Map<number, Known>();
  /** Salts the hashes kept of passwords, so they can't be looked up. */
  private readonly salt = nacl.randomBytes(32);

  constructor(
    private readonly client: KeefastClient,
    private readonly tabs: Tabs,
  ) {}

  /**
   * The Entries for the page in `tab`, or none if the extension can't fill the page. Entries
   * listed for the page's site a moment ago are listed again; otherwise keefast is asked, and
   * it may ask the user first. Fails if keefast is locked or not running.
   */
  async entries(tab: number): Promise<EntrySummary[] | undefined> {
    const entries = await this.entriesFor(tab);
    return entries?.map(({ uuid, name, username }) => ({ uuid, name, username }));
  }

  /** Fills the listed Entry `uuid` into `tab`, and says whether it did. */
  async fill(tab: number, uuid: string): Promise<boolean> {
    const listed = this.listed.get(tab);
    const entry = listed?.entries.find((entry) => entry.uuid === uuid);
    if (!listed || !entry) return false;
    this.forget(tab);
    const filled = await this.tabs.fill(tab, listed.origin, entry);
    const known = this.known.get(tab);
    if (filled && known?.origin === listed.origin) known.filled = entry.uuid;
    return filled;
  }

  /**
   * Fills the page's only Entry. With several, none, or keefast unavailable, the user should
   * see the popup to choose, or to learn what's wrong.
   */
  async shortcut(tab: number): Promise<"filled" | "choose"> {
    try {
      const entries = await this.entriesFor(tab);
      if (entries?.length === 1 && (await this.fill(tab, entries[0]!.uuid))) {
        return "filled";
      }
    } catch (error) {
      if (!(error instanceof KeefastError)) throw error;
    }
    return "choose";
  }

  /**
   * Offers keefast a login the user submitted on `page`, in `tab`, for the user to save there,
   * unless it is an Entry keefast listed for the site, or was offered already. It updates the
   * Entry it changes the password of, when the extension knows which. Waits for the user.
   */
  async submitted(tab: number, page: Page, login: SubmittedLogin): Promise<void> {
    const known = this.knownAt(tab, page.origin);
    const fingerprint = this.fingerprint(login.username, login.password);
    if (known.offered.has(fingerprint) || this.isListed(known, login)) return;
    const updates = this.updatedEntry(known, login);
    known.offered.add(fingerprint);
    try {
      const saved = await this.client.saveLogin({
        url: page.url,
        submitUrl: page.submitUrl,
        username: login.username || (updates?.username ?? ""),
        password: login.password,
        uuid: updates?.uuid,
      });
      // The site's Entries listed a moment ago no longer are all it has.
      if (saved) this.forget(tab);
    } catch (error) {
      // Not offered after all: it may be once keefast is unlocked.
      known.offered.delete(fingerprint);
      throw error;
    }
  }

  /** Whether a sign-in is with an Entry listed for the site, as it is. */
  private isListed(known: Known, { username, password, current }: SubmittedLogin): boolean {
    if (current !== undefined) return false;
    const login = this.fingerprint(username, password);
    const alone = this.fingerprint("", password);
    return known.entries.some(
      (entry) => entry.login === login || (username === "" && entry.password === alone),
    );
  }

  /**
   * The listed Entry a login updates: the one whose password a password change replaces, or
   * else the one with its username, or, without a username, the one just filled.
   */
  private updatedEntry(known: Known, { username, current }: SubmittedLogin) {
    const replaced = current === undefined ? undefined : this.fingerprint("", current);
    return (
      known.entries.find((entry) => entry.password === replaced) ??
      known.entries.find((entry) =>
        username === "" ? entry.uuid === known.filled : entry.username === username,
      )
    );
  }

  private async entriesFor(tab: number): Promise<Entry[] | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    const listed = this.listed.get(tab);
    if (listed?.origin === page.origin) return listed.entries;
    const entries = await this.client.entries(page.url, page.submitUrl);
    this.forget(tab);
    this.knownAt(tab, page.origin).entries = entries.map(({ uuid, username, password }) => ({
      uuid,
      username,
      login: this.fingerprint(username, password),
      password: this.fingerprint("", password),
    }));
    this.listed.set(tab, {
      origin: page.origin,
      entries,
      expiry: setTimeout(() => this.forget(tab), KEEP_ENTRIES_MS),
    });
    return entries;
  }

  /** What the extension knows of the site at `origin` in `tab`: nothing, once it changed. */
  private knownAt(tab: number, origin: string): Known {
    const previous = this.known.get(tab);
    if (previous) clearTimeout(previous.expiry);
    const known: Known =
      previous?.origin === origin ? previous : { origin, entries: [], offered: new Set() };
    known.expiry = setTimeout(() => this.known.delete(tab), KNOW_ENTRIES_MS);
    this.known.set(tab, known);
    return known;
  }

  /** A salted hash of a login, to compare it without keeping its password. */
  private fingerprint(username: string, password: string): string {
    const bytes = new TextEncoder().encode(JSON.stringify([username, password]));
    const salted = new Uint8Array(this.salt.length + bytes.length);
    salted.set(this.salt);
    salted.set(bytes, this.salt.length);
    return encodeBase64(nacl.hash(salted));
  }

  private forget(tab: number): void {
    const listed = this.listed.get(tab);
    if (listed) clearTimeout(listed.expiry);
    this.listed.delete(tab);
  }
}
