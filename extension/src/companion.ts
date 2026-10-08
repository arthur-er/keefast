// What the popup and the shortcut do: ask keefast for the page's Entries, and fill the one the
// user picks. Passwords stay here, never in the popup, and only until filled.
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

interface Listed {
  origin: string;
  entries: Entry[];
  expiry: ReturnType<typeof setTimeout>;
}

export class Companion {
  private listed = new Map<number, Listed>();

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
    return this.tabs.fill(tab, listed.origin, entry);
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

  private async entriesFor(tab: number): Promise<Entry[] | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    const listed = this.listed.get(tab);
    if (listed?.origin === page.origin) return listed.entries;
    const entries = await this.client.entries(page.url, page.submitUrl);
    this.forget(tab);
    this.listed.set(tab, {
      origin: page.origin,
      entries,
      expiry: setTimeout(() => this.forget(tab), KEEP_ENTRIES_MS),
    });
    return entries;
  }

  private forget(tab: number): void {
    const listed = this.listed.get(tab);
    if (listed) clearTimeout(listed.expiry);
    this.listed.delete(tab);
  }
}
