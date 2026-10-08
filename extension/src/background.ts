// The service worker: it holds the connection to keefast, and answers the popup and the
// shortcut. Nothing runs in pages until the user asks to fill one.
import { Companion, type Tabs } from "./companion";
import { HOST_NAME, KeefastClient, KeefastError, type NativePort, type Pairing } from "./keefast";
import type { Answer, Answers, Request } from "./messages";

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
async function inPage<T>(tab: number, func: (...args: string[]) => T, args: string[] = []) {
  await chrome.scripting.executeScript({ target: { tabId: tab }, files: ["content.js"] });
  const [result] = await chrome.scripting.executeScript({ target: { tabId: tab }, func, args });
  return result?.result as Awaited<T> | undefined;
}

const tabs: Tabs = {
  async inspect(tab) {
    try {
      return (await inPage(tab, () => globalThis.keefast?.inspect())) ?? undefined;
    } catch {
      // The browser's own pages, and pages the user didn't open the popup on, can't be filled.
      return undefined;
    }
  },
  async fill(tab, origin, { login, password }) {
    try {
      const filled = await inPage(
        tab,
        (origin, login, password) => globalThis.keefast?.fill(origin!, login!, password!) ?? false,
        [origin, login, password],
      );
      return filled === true;
    } catch {
      return false;
    }
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
    case "logins":
      return (await companion.logins(request.tab)) ?? null;
    case "fill":
      return companion.fill(request.tab, request.uuid);
  }
}

chrome.runtime.onMessage.addListener((request: Request, sender, respond) => {
  // Only the extension's own pages: content scripts don't send messages.
  if (sender.id !== chrome.runtime.id || sender.tab) return false;
  answer(request).then(
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
  if (command !== "fill-login" || tab?.id === undefined) return;
  if ((await companion.shortcut(tab.id)) === "choose") {
    // The popup lists the logins to choose from, or says what's wrong.
    await chrome.action.openPopup().catch(() => {});
  }
});
