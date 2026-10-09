// The service worker: it holds the connection to keefast, answers the popup and the shortcut,
// and offers keefast the logins submitted in pages. Nothing runs in pages until the user asks
// to fill one.
import { Companion, type Tabs } from "./companion";
import { HOST_NAME, KeefastClient, KeefastError, type NativePort, type Pairing } from "./keefast";
import type { Credentials } from "./forms";
import type { Answer, Answers, PageMessage, Request } from "./messages";

function connectNative(): NativePort {
  const port = chrome.runtime.connectNative(HOST_NAME);
  return {
    postMessage: (message) => port.postMessage(message),
    onMessage: (listener) => port.onMessage.addListener(listener),
    onDisconnect: (listener) =>
      port.onDisconnect.addListener(() => {
        // Reading the error marks it handled: a host that isn't installed is expected.
        void chrome.runtime.lastError;
        listener();
      }),
    disconnect: () => port.disconnect(),
  };
}

const pairings = {
  async load(): Promise<Pairing[]> {
    const { pairings } = await chrome.storage.local.get("pairings");
    return Array.isArray(pairings) ? (pairings as Pairing[]) : [];
  },
  async save(pairings: Pairing[]): Promise<void> {
    await chrome.storage.local.set({ pairings });
  },
};

/** Runs `func` in the tab's top frame, with the page script injected first. */
async function inPage<Args extends unknown[], T>(
  tab: number,
  func: (...args: Args) => T,
  args: Args,
) {
  await chrome.scripting.executeScript({ target: { tabId: tab }, files: ["content.js"] });
  const [result] = await chrome.scripting.executeScript({ target: { tabId: tab }, func, args });
  return result?.result as Awaited<T> | undefined;
}

/** Runs a fill in the tab's page, and says whether it filled anything. */
async function fillIn<Args extends unknown[]>(
  tab: number,
  func: (...args: Args) => boolean,
  args: Args,
): Promise<boolean> {
  try {
    return (await inPage(tab, func, args)) === true;
  } catch {
    return false;
  }
}

const tabs: Tabs = {
  async inspect(tab) {
    try {
      return (await inPage(tab, () => globalThis.keefast?.inspect(), [])) ?? undefined;
    } catch {
      // The browser's own pages, and pages the user didn't open the popup on, can't be filled.
      return undefined;
    }
  },
  fill(tab, origin, credentials) {
    return fillIn(
      tab,
      (origin, credentials) => globalThis.keefast?.fill(origin, credentials) ?? false,
      [origin, credentials] as [string, Credentials],
    );
  },
  fillNewPassword(tab, origin, password) {
    return fillIn(
      tab,
      (origin, password) => globalThis.keefast?.fillNewPassword(origin, password) ?? false,
      [origin, password] as [string, string],
    );
  },
};

const client = new KeefastClient(connectNative, pairings);
const companion = new Companion(client, tabs);

async function answer(request: Request): Promise<Answers[Request["type"]]> {
  switch (request.type) {
    case "status":
      return client.status();
    case "pair":
      await client.pair();
      return null;
    case "entries":
      return (await companion.entries(request.tab)) ?? null;
    case "fill":
      return companion.fill(request.tab, request.uuid);
    case "generate":
      return (await companion.generate(request.tab)) ?? null;
    case "fill-generated":
      return companion.fillGenerated(request.tab);
  }
}

/** Offers keefast a login submitted in a page the user filled. */
function submitted(message: unknown, sender: chrome.runtime.MessageSender): void {
  const tab = sender.tab?.id;
  // Only the page the extension was injected into: the tab's top frame.
  if (tab === undefined || sender.frameId !== 0 || !sender.url || !isSubmitted(message)) return;
  const url = new URL(sender.url);
  if (url.protocol !== "https:" && url.protocol !== "http:") return;
  // The browser says which site the login was submitted on, not the page.
  const page = { url: sender.url, origin: url.origin, submitUrl: message.submitUrl };
  // When keefast is locked or not running, the login can't be saved: there's no one to ask.
  companion.submitted(tab, page, message.login).catch(() => {});
}

function isSubmitted(message: unknown): message is PageMessage {
  if (typeof message !== "object" || message === null) return false;
  const { type, login, submitUrl } = message as Partial<PageMessage>;
  return (
    type === "submitted" &&
    typeof login === "object" &&
    login !== null &&
    typeof login.username === "string" &&
    typeof login.password === "string" &&
    (login.current === undefined || typeof login.current === "string") &&
    (submitUrl === undefined || typeof submitUrl === "string")
  );
}

chrome.runtime.onMessage.addListener((request: unknown, sender, respond) => {
  if (sender.id !== chrome.runtime.id) return false;
  if (sender.tab) {
    submitted(request, sender);
    return false;
  }
  answer(request as Request).then(
    (value) => respond({ ok: true, value } satisfies Answer<unknown>),
    (error: unknown) =>
      respond({
        ok: false,
        reason: error instanceof KeefastError ? error.reason : "failed",
      } satisfies Answer<unknown>),
  );
  return true;
});

chrome.commands.onCommand.addListener(async (command, tab) => {
  if (command !== "fill" || tab?.id === undefined) return;
  const outcome = await companion.shortcut(tab.id).catch(() => "choose" as const);
  if (outcome === "choose") {
    // The popup lists the Entries to choose from, or says what's wrong.
    await chrome.action.openPopup().catch(() => {});
  }
});
