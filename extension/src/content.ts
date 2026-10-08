// Injected into the page only when the user asks to fill it, from the popup or the shortcut.
import { fillLogin, findLoginFields } from "./forms";
import type { Page } from "./companion";

export interface PageApi {
  /** The page, or none if the extension doesn't fill pages like it. */
  inspect(): Page | undefined;
  fill(origin: string, username: string, password: string): boolean;
}

declare global {
  // In the extension's isolated world: the page's own scripts can't see it.
  var keefast: PageApi | undefined;
}

globalThis.keefast = {
  inspect() {
    if (location.protocol !== "https:" && location.protocol !== "http:") return undefined;
    return {
      url: location.href,
      origin: location.origin,
      submitUrl: findLoginFields(document)?.submitUrl,
    };
  },
  fill(origin, username, password) {
    return fillLogin(document, origin, username, password);
  },
};
