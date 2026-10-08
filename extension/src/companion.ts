// What the popup and the shortcut do: ask keefast for the page's logins, and fill the one the
// user picks. Passwords stay here, never in the popup, and only until filled.
import { KeefastError, type KeefastClient, type Login } from "./keefast";

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
  /** Fills a login into the tab's login form, if the tab is still at `origin`. */
  fill(tab: number, origin: string, login: Login): Promise<boolean>;
}

/** A login as the popup lists it, without its password. */
export interface LoginSummary {
  uuid: string;
  name: string;
  login: string;
}

/** How long logins keefast gave for a tab are kept for the user to pick one. */
const KEEP_LOGINS_MS = 2 * 60_000;

interface Listed {
  origin: string;
  logins: Login[];
  expiry: ReturnType<typeof setTimeout>;
}

export class Companion {
  private listed = new Map<number, Listed>();

  constructor(
    private readonly client: KeefastClient,
    private readonly tabs: Tabs,
  ) {}

  /**
   * The logins for the page in `tab`, or none if the extension can't fill the page. Logins
   * listed for the page's site a moment ago are listed again; otherwise keefast is asked, and
   * it may ask the user first. Fails if keefast is locked or not running.
   */
  async logins(tab: number): Promise<LoginSummary[] | undefined> {
    const logins = await this.fetch(tab);
    return logins?.map(({ uuid, name, login }) => ({ uuid, name, login }));
  }

  /** Fills the listed login `uuid` into `tab`, and says whether it did. */
  async fill(tab: number, uuid: string): Promise<boolean> {
    const listed = this.listed.get(tab);
    const login = listed?.logins.find((login) => login.uuid === uuid);
    if (!listed || !login) return false;
    this.forget(tab);
    return this.tabs.fill(tab, listed.origin, login);
  }

  /**
   * Fills the page's only login. With several, none, or keefast unavailable, the user should
   * see the popup to choose, or to learn what's wrong.
   */
  async shortcut(tab: number): Promise<"filled" | "choose"> {
    try {
      const logins = await this.fetch(tab);
      if (logins?.length === 1 && (await this.fill(tab, logins[0]!.uuid))) {
        return "filled";
      }
    } catch (error) {
      if (!(error instanceof KeefastError)) throw error;
    }
    return "choose";
  }

  private async fetch(tab: number): Promise<Login[] | undefined> {
    const page = await this.tabs.inspect(tab);
    if (!page) return undefined;
    const listed = this.listed.get(tab);
    if (listed?.origin === page.origin) return listed.logins;
    const logins = await this.client.logins(page.url, page.submitUrl);
    this.forget(tab);
    this.listed.set(tab, {
      origin: page.origin,
      logins,
      expiry: setTimeout(() => this.forget(tab), KEEP_LOGINS_MS),
    });
    return logins;
  }

  private forget(tab: number): void {
    const listed = this.listed.get(tab);
    if (listed) clearTimeout(listed.expiry);
    this.listed.delete(tab);
  }
}
