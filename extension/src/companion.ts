// What the popup, the command palette, the icons in sign-in fields and the shortcut do: ask
// keefast for the page's Entries, and fill the one the user picks, with its TOTP code on a 2FA
// step; search the paired Vaults for one when none is for the site, and fill the one picked; or
// fill a password keefast's generator made. Entries' passwords and TOTP codes stay here, never
// in the popup or the palette, and only until filled; they show only a generated password, for
// the user to see what they sign up with. Then, what the page does: offer keefast the logins
// the user submits there that it doesn't have yet. On a payment form, list the Entries with a
// Card and fill the one the user picks (ADR 0010), whose values likewise stay here until filled.
import nacl from "tweetnacl";
import type { SubmittedLogin } from "./capture";
import { encodeBase64 } from "./crypto";
import { type Credentials, type PageFields, isTwoFactorStep } from "./forms";
import { KeefastError, type Entry, type FoundEntry, type KeefastClient } from "./keefast";
import { type CardValues, type PaymentToken, valuesFor } from "./payment";

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

/** A document in a tab, the page or one of its frames, with payment fields. */
export interface PaymentFrame {
  /** The browser's id of the document: filling it fails once it has navigated. */
  documentId: string;
  frameId: number;
  url: string;
  origin: string;
  /** What its payment fields take. */
  tokens: PaymentToken[];
}

/** A frame the user clicked keefast's icon in, whose origin may get a Card's values. */
export interface ChosenFrame {
  url: string;
  origin: string;
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
  /** The documents in the tab with payment fields the extension may read: the page, and frames. */
  paymentFrames(tab: number): Promise<PaymentFrame[]>;
  /** Fills a Card's values into a document's payment fields, if it is still at its origin. */
  fillPayment(tab: number, frame: PaymentFrame, values: CardValues): Promise<boolean>;
}

/** An Entry as the popup and the palette list it, without its password. */
export interface EntrySummary {
  uuid: string;
  name: string;
  username: string;
  hasTotp: boolean;
  /** The name of the Group the Entry is in. */
  group: string;
}

/** The Entries that can fill a page, and what the page has to fill. */
export interface PageEntries {
  entries: EntrySummary[];
  fields: PageFields;
}

/** How long the Entries keefast gave for a tab are kept for the user to pick one. */
const KEEP_ENTRIES_MS = 2 * 60_000;

/** How long a password keefast's generator made for a tab is kept for the user to fill it. */
const KEEP_GENERATED_MS = 2 * 60_000;

/**
 * How long the extension remembers which Entries a tab's site has, to tell a login submitted
 * there from one keefast already has.
 */
const KNOW_ENTRIES_MS = 15 * 60_000;

/** Kept for a tab until its expiry. */
interface Kept {
  expiry: ReturnType<typeof setTimeout>;
}

interface Listed extends Kept {
  origin: string;
  entries: Entry[];
  /** What the page had to fill when last looked at. */
  fields: PageFields;
}

/** A password keefast's generator made for the page at `origin`, until filled there. */
interface Generated extends Kept {
  origin: string;
  password: string;
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
      entries: entriesFilling(listed).map(({ uuid, name, username, hasTotp, group }) => ({
        uuid,
        name,
        username,
        hasTotp,
        group,
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
      if (credentials.totp === undefined && isTwoFactorStep(fields)) return false;
    }
    const filled = await this.tabs.fill(tab, origin, credentials);
    const known = this.known.get(tab);
    if (filled && known?.origin === origin) known.filled = entry.uuid;
    return filled;
  }

  /**
   * The Entries in the paired Vaults matching `query`, for the user to pick one for the page in
   * `tab` in the command palette; none if the extension can't fill the page.
   */
  async search(tab: number, query: string): Promise<FoundEntry[] | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    if (query.trim() === "") return [];
    return this.client.search(page.url, page.submitUrl, query);
  }

  /**
   * Fills the Entry `uuid` the user picked in the command palette, found by {@link search},
   * into `tab`, with its TOTP code if the page has a 2FA field, and says whether it did.
   * keefast remembers the pick for the site, and with `addUrl` adds the site to the Entry.
   */
  async fillPicked(tab: number, uuid: string, addUrl: boolean): Promise<boolean> {
    const page = await this.tabs.inspect(tab);
    if (!page) return false;
    const entry = await this.client.pick(uuid, page.url, page.submitUrl, addUrl);
    if (!entry) return false;
    // The site's Entries listed a moment ago may no longer be all it has.
    this.forget(tab);
    const { origin, fields } = page;
    const credentials: Credentials = { username: entry.username, password: entry.password };
    // The code came with the Entry, a moment ago.
    if (fields.totp && entry.totp !== undefined) credentials.totp = entry.totp;
    if (isTwoFactorStep(fields) && credentials.totp === undefined) return false;
    const filled = await this.tabs.fill(tab, origin, credentials);
    const known = this.knownAt(tab, origin);
    if (!known.entries.some((other) => other.uuid === entry.uuid)) {
      known.entries.push(this.knownEntry(entry));
    }
    if (filled) known.filled = entry.uuid;
    return filled;
  }

  /**
   * The Entries with a Card in the paired Vaults matching `query`, all of them for an empty one,
   * for the user to pick one in the command palette over the page in `tab`; none if the
   * extension can't fill the page.
   */
  async cards(tab: number, query: string): Promise<FoundEntry[] | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    return this.client.searchForm("payment", page.url, page.submitUrl, query);
  }

  /**
   * Fills the Card of the Entry `uuid`, which the user picked in the command palette, into the
   * payment fields in `tab`: the page's own, and those of frames from the page's origin or from
   * `frame`'s, the one the user clicked keefast's icon in. A frame from any other origin gets
   * nothing, and each gets only the values its fields take. keefast is asked only when there is
   * a field to fill; it remembers the pick for the site and the frame. Says whether it filled any.
   */
  async fillCard(tab: number, uuid: string, frame?: ChosenFrame): Promise<boolean> {
    const page = await this.tabs.inspect(tab);
    if (!page) return false;
    const origins = new Set([page.origin, frame?.origin ?? page.origin]);
    const targets = (await this.tabs.paymentFrames(tab)).filter(
      (target) => origins.has(target.origin) && target.tokens.length > 0,
    );
    if (targets.length === 0) return false;
    const card = await this.client.fillForm(
      "payment",
      uuid,
      page.url,
      page.submitUrl,
      frame?.url ?? page.url,
    );
    if (!card) return false;
    let filled = false;
    for (const target of targets) {
      const values = valuesFor(target.tokens, card.values);
      if (Object.keys(values).length === 0) continue;
      if (await this.tabs.fillPayment(tab, target, values)) filled = true;
    }
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
    drop(this.generated, tab);
    this.generated.set(tab, {
      origin: page.origin,
      password,
      expiry: setTimeout(() => drop(this.generated, tab), KEEP_GENERATED_MS),
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
    drop(this.generated, tab);
    return this.tabs.fillNewPassword(tab, generated.origin, generated.password);
  }

  /**
   * Fills the page's only Entry. With several, none, or keefast unavailable, the user should
   * see the palette or the popup to choose, or to learn what's wrong.
   */
  async shortcut(tab: number): Promise<"filled" | "choose"> {
    try {
      const listed = await this.listFor(tab);
      const entries = listed && entriesFilling(listed);
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
    this.knownAt(tab, page.origin).entries = entries.map((entry) => this.knownEntry(entry));
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

  private knownEntry({ uuid, username, password }: Entry): KnownEntry {
    return {
      uuid,
      username,
      loginHash: this.fingerprint(username, password),
      passwordHash: this.fingerprint("", password),
    };
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
    drop(this.listed, tab);
  }
}

/** Drops what is kept for `tab`, before its expiry. */
function drop(kept: Map<number, Kept>, tab: number): void {
  const item = kept.get(tab);
  if (item) clearTimeout(item.expiry);
  kept.delete(tab);
}

/** The listed Entries that can fill the page: on a 2FA step, those with TOTP. */
function entriesFilling({ entries, fields }: Listed): Entry[] {
  return isTwoFactorStep(fields) ? entries.filter((entry) => entry.hasTotp) : entries;
}
