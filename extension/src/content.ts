// Injected into the page only when the user asks to fill it, from the popup or the shortcut.
// From then on, until the page goes, it also tells the extension the logins submitted there.
import { type SubmittedLogin, submitsLogin, submittedLogin } from "./capture";
import {
  type Credentials,
  fillLogin,
  fillNewPassword,
  fillTotp,
  findLoginFields,
  pageFields,
} from "./forms";
import type { InspectedPage } from "./companion";
import type { PageMessage } from "./messages";

export interface PageApi {
  /** The page, or none if the extension doesn't fill pages like it. */
  inspect(): InspectedPage | undefined;
  /** Fills the login form, and the 2FA field with the TOTP code; says whether it filled any. */
  fill(origin: string, credentials: Credentials): boolean;
  fillNewPassword(origin: string, password: string): boolean;
}

declare global {
  // In the extension's isolated world: the page's own scripts can't see it.
  var keefast: PageApi | undefined;
}

/** Tells the extension the login submitted with `scope`, if there is one. */
function tell(scope: HTMLFormElement | Document): void {
  const login: SubmittedLogin | undefined = submittedLogin(scope);
  if (!login) return;
  const message: PageMessage = {
    type: "submitted",
    login,
    submitUrl: scope instanceof HTMLFormElement ? scope.action : undefined,
  };
  // The extension may be gone, reloaded or updated, meanwhile.
  chrome.runtime.sendMessage(message).catch(() => {});
}

/**
 * Whether the user just did this: the page's own scripts can submit forms and send events
 * too, to get a password of their choosing offered as an Entry's.
 */
function byUser(event: Event): boolean {
  return event.isTrusted && navigator.userActivation.isActive;
}

/**
 * Watches for logins the user submits: by a form, or, for pages that sign in by script, by a
 * button that says it submits, or Enter in a password field. Telling the same login twice
 * is fine: the extension offers it once.
 */
function watchSubmissions(): void {
  const scopeOf = (element: Element) => element.closest("form") ?? document;
  window.addEventListener(
    "submit",
    (event) => {
      if (byUser(event) && event.target instanceof HTMLFormElement) tell(event.target);
    },
    true,
  );
  window.addEventListener(
    "click",
    (event) => {
      if (byUser(event) && event.target instanceof Element && submitsLogin(event.target)) {
        tell(scopeOf(event.target));
      }
    },
    true,
  );
  window.addEventListener(
    "keydown",
    (event) => {
      const input = event.target;
      if (
        byUser(event) &&
        event.key === "Enter" &&
        input instanceof HTMLInputElement &&
        input.type === "password"
      ) {
        tell(scopeOf(input));
      }
    },
    true,
  );
}

// Injected again on every fill: the page keeps one watcher.
if (!globalThis.keefast) watchSubmissions();

globalThis.keefast = {
  inspect() {
    if (location.protocol !== "https:" && location.protocol !== "http:") return undefined;
    return {
      url: location.href,
      origin: location.origin,
      submitUrl: findLoginFields(document)?.submitUrl,
      fields: pageFields(document),
    };
  },
  fill(origin, credentials) {
    const login = fillLogin(document, origin, credentials);
    const totp = credentials.totp !== undefined && fillTotp(document, origin, credentials.totp);
    return login || totp;
  },
  fillNewPassword(origin, password) {
    return fillNewPassword(document, origin, password);
  },
};
