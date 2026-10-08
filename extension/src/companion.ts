// What the popup and the shortcut do: ask keefast for the page's Entries, and fill the one the
// user picks, with its TOTP code on a 2FA step; or fill a password keefast's generator made.
// Passwords stay here, never in the popup, and only until filled. Then, what the page does:
// offer keefast the logins the user submits there that it doesn't have yet.
import nacl from "tweetnacl";
import type { SubmittedLogin } from "./capture";
import { encodeBase64 } from "./crypto";
import type { Credentials, PageFields } from "./forms";
import { KeefastError, type Entry, type KeefastClient } from "./keefast";

/** A page the extension may fill, as its login form sees it. */
export interface Page {
  url: string;
  origin: string;
  /** Where the page's login form submits to. */
  submitUrl?: string;
}

/** A page as the extension finds it when the user asks to fill it. */
export interface InspectedPage extends Page {
  /** What the page has to fill. */
  fields: PageFields;
}

/** The browser's tabs, as far as filling them goes. */
export interface Tabs {
  /** The page in a tab, or none if the extension can't fill it. */
  inspect(tab: number): Promise<InspectedPage | undefined>;
  /**
   * Fills an Entry into the tab's login form, and its TOTP code into a 2FA field, if the tab is
   * still at `origin`.
   */
  fill(tab: number, origin: string, credentials: Credentials): Promise<boolean>;
  /** Fills a new password into the tab's new password fields, if the tab is still at `origin`. */
  fillNewPassword(tab: number, origin: string, password: string): Promise<boolean>;
}

/** An Entry as the popup lists it, without its password. */
export interface EntrySummary {
  uuid: string;
  name: string;
  username: string;
  hasTotp: boolean;
}

/** The Entries that can fill a page, and what the page has to fill. */
export interface PageEntries {
  entries: EntrySummary[];
  fields: PageFields;
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
  /** What the page had to fill when last looked at. */
  fields: PageFields;
  expiry: ReturnType<typeof setTimeout>;
}

/** A password keefast's generator made for the page at `origin`, until filled there. */
interface Generated {
  origin: string;
  password: string;
  expiry: ReturnType<typeof setTimeout>;
}

/** An Entry the extension listed for a site, without its password. */
interface KnownEntry {
  uuid: string;
  username: string;
  /** The Entry's username and password, as {@link Companion.fingerprint} hashes them. */
  loginHash: string;
  /** Its password alone, hashed likewise, to match the password a password change replaces. */
  passwordHash: string;
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
  private generated = new Map<number, Generated>();
  /** Salts the hashes kept of passwords, so they can't be looked up. */
  private readonly salt = nacl.randomBytes(32);

  constructor(
    private readonly client: KeefastClient,
    private readonly tabs: Tabs,
  ) {}

  /**
   * The Entries that can fill the page in `tab`, or none if the extension can't fill the page.
   * On a 2FA step, those are the Entries with TOTP. Entries listed for the page's site a moment
   * ago are listed again; otherwise keefast is asked, and it may ask the user first. Fails if
   * keefast is locked or not running.
   */
  async entries(tab: number): Promise<PageEntries | undefined> {
    const listed = await this.listFor(tab);
    if (!listed) return undefined;
    return {
      entries: fillers(listed).map(({ uuid, name, username, hasTotp }) => ({
        uuid,
        name,
        username,
        hasTotp,
      })),
      fields: listed.fields,
    };
  }

  /**
   * Fills the listed Entry `uuid` into `tab`, with its current TOTP code if the page has a 2FA
   * field, and says whether it did.
   */
  async fill(tab: number, uuid: string): Promise<boolean> {
    const listed = this.listed.get(tab);
    const entry = listed?.entries.find((entry) => entry.uuid === uuid);
    if (!listed || !entry) return false;
    this.forget(tab);
    const { origin, fields } = listed;
    const credentials: Credentials = { username: entry.username, password: entry.password };
    if (fields.totp && entry.hasTotp) {
      // Asked now, not taken from the listing: the code changes every 30 seconds or so.
      credentials.totp = await this.client.totp(entry.uuid);
      // keefast gives it only while the Entry is the site's: listing it again may tell.
      if (credentials.totp === undefined && !fields.login) return false;
    }
    const filled = await this.tabs.fill(tab, origin, credentials);
    const known = this.known.get(tab);
    if (filled && known?.origin === origin) known.filled = entry.uuid;
    return filled;
  }

  /**
   * A new password from keefast's generator, for the page in `tab`, or none if the extension
   * can't fill the page. It is kept for a while for {@link fillGenerated} to fill.
   */
  async generate(tab: number): Promise<string | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    const password = await this.client.generatePassword();
    this.forgetGenerated(tab);
    this.generated.set(tab, {
      origin: page.origin,
      password,
      expiry: setTimeout(() => this.forgetGenerated(tab), KEEP_ENTRIES_MS),
    });
    return password;
  }

  /**
   * Fills the password last generated for `tab` into its new password fields, if the tab is
   * still at the site it was made for, and says whether it did.
   */
  async fillGenerated(tab: number): Promise<boolean> {
    const generated = this.generated.get(tab);
    if (!generated) return false;
    this.forgetGenerated(tab);
    return this.tabs.fillNewPassword(tab, generated.origin, generated.password);
  }

  /**
   * Fills the page's only Entry. With several, none, or keefast unavailable, the user should
   * see the popup to choose, or to learn what's wrong.
   */
  async shortcut(tab: number): Promise<"filled" | "choose"> {
    try {
      const listed = await this.listFor(tab);
      const entries = listed && fillers(listed);
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
      (entry) => entry.loginHash === login || (username === "" && entry.passwordHash === alone),
    );
  }

  /**
   * The listed Entry a login updates: the one whose password a password change replaces, or
   * else the one with its username. Without a username, the one just filled, or else the
   * site's only Entry on a password change.
   */
  private updatedEntry(known: Known, { username, current }: SubmittedLogin) {
    const { entries, filled } = known;
    if (current !== undefined) {
      const replaced = this.fingerprint("", current);
      const entry = entries.find((entry) => entry.passwordHash === replaced);
      if (entry) return entry;
    }
    if (username !== "") return entries.find((entry) => entry.username === username);
    const entry = entries.find((entry) => entry.uuid === filled);
    if (entry) return entry;
    return current !== undefined && entries.length === 1 ? entries[0] : undefined;
  }

  /** The Entries listed for the page in `tab`, asking keefast unless they were a moment ago. */
  private async listFor(tab: number): Promise<Listed | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    const previous = this.listed.get(tab);
    if (previous?.origin === page.origin) {
      // The site's next step, such as its 2FA step, may be at the same origin.
      previous.fields = page.fields;
      return previous;
    }
    const entries = await this.client.entries(page.url, page.submitUrl);
    this.forget(tab);
    this.knownAt(tab, page.origin).entries = entries.map(({ uuid, username, password }) => ({
      uuid,
      username,
      loginHash: this.fingerprint(username, password),
      passwordHash: this.fingerprint("", password),
    }));
    const listed: Listed = {
      origin: page.origin,
      entries,
      fields: page.fields,
      expiry: setTimeout(() => this.forget(tab), KEEP_ENTRIES_MS),
    };
    this.listed.set(tab, listed);
    return listed;
  }

  /**
   * What the extension knows of the site at `origin` in `tab`, kept a while longer: afresh
   * when the tab has moved to another site.
   */
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

  private forgetGenerated(tab: number): void {
    const generated = this.generated.get(tab);
    if (generated) clearTimeout(generated.expiry);
    this.generated.delete(tab);
  }
}

/** The listed Entries that can fill the page: on a 2FA step alone, those with TOTP. */
function fillers({ entries, fields }: Listed): Entry[] {
  return fields.totp && !fields.login ? entries.filter((entry) => entry.hasTotp) : entries;
}
