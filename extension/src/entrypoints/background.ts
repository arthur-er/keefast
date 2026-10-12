// The service worker: it holds the connection to keefast, answers the popup, the command
// palette, keefast's icons in sign-in and payment fields and the shortcut, and offers keefast the
// logins submitted in pages. Pages get keefast's icons only once the user turns them on (ADR 0007);
// otherwise nothing runs in pages until the user asks to fill one.
import { browser, type Browser } from "wxt/browser";
import { defineBackground } from "wxt/utils/define-background";
import { type ChosenFrame, Companion, type PaymentFrame, type Tabs } from "../companion";
import type { Credentials } from "../forms";
import { KeefastClient, KeefastError, HOST_NAME, type NativePort } from "../keefast";
import type { CardValues } from "../payment";
import type {
  Answer,
  Answers,
  FieldClickAnswer,
  FieldKind,
  PageAnswers,
  PageMessage,
  PaletteAnswers,
  PaletteMessage,
  PaletteMode,
  PaletteRequest,
  Request,
  TabMessage,
} from "../messages";
import { ALL_SITES, FIELDS_SCRIPT, addSiteOnPick, hiddenSites, pairings } from "../settings";

type MessageSender = Browser.runtime.MessageSender;

/** The page script, which finds and fills a page's fields, and frames the command palette. */
const PAGE_SCRIPT = "/page.js";
/** The script that finds and fills payment fields, in the page and its frames. */
const PAYMENT_SCRIPT = "/payment.js";
const PALETTE_PAGE = "palette.html";

function connectNative(): NativePort {
  const port = browser.runtime.connectNative(HOST_NAME);
  return {
    postMessage: (message) => port.postMessage(message),
    onMessage: (listener) => port.onMessage.addListener(listener),
    onDisconnect: (listener) =>
      port.onDisconnect.addListener(() => {
        // Reading the error marks it handled: a host that isn't installed is expected.
        void browser.runtime.lastError;
        listener();
      }),
    disconnect: () => port.disconnect(),
  };
}

const pairingStore = {
  load: () => pairings.getValue(),
  save: (value: Parameters<typeof pairings.setValue>[0]) => pairings.setValue(value),
};

/** Runs `func` in the tab's top frame, with the page script injected first. */
async function inPage<Args extends unknown[], T>(
  tab: number,
  func: (...args: Args) => T,
  args: Args,
) {
  await browser.scripting.executeScript({ target: { tabId: tab }, files: [PAGE_SCRIPT] });
  const [result] = await browser.scripting.executeScript({ target: { tabId: tab }, func, args });
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
  async paymentFrames(tab) {
    // Every frame the extension may read: with keefast in fields on, the frames of every origin;
    // otherwise, the page alone, as the user opened it.
    const inject = async (allFrames: boolean) => {
      await browser.scripting.executeScript({ target: { tabId: tab, allFrames }, files: [PAYMENT_SCRIPT] });
      return browser.scripting.executeScript({
        target: { tabId: tab, allFrames },
        func: () => globalThis.keefastPayment?.inspect(),
      });
    };
    const results = await inject(true).catch(() => inject(false).catch(() => []));
    return results.flatMap(({ documentId, frameId, result }): PaymentFrame[] =>
      result && documentId ? [{ documentId, frameId, ...result }] : [],
    );
  },
  async fillPayment(tab, frame, values) {
    try {
      // Only the document found: a frame that navigated since is another document.
      const [result] = await browser.scripting.executeScript({
        target: { tabId: tab, documentIds: [frame.documentId] },
        func: (origin: string, values: CardValues) =>
          globalThis.keefastPayment?.fill(origin, values) ?? false,
        args: [frame.origin, values],
      });
      return result?.result === true;
    } catch {
      return false;
    }
  },
};

const client = new KeefastClient(connectNative, pairingStore);
const companion = new Companion(client, tabs);

/** A command palette open in a tab, which only its token opens to. */
interface OpenPalette {
  token: string;
  mode: PaletteMode;
  origin: string;
  /** For a Card, the frame the user clicked keefast's icon in, when it isn't the page. */
  frame?: ChosenFrame;
}

/** The palette open in each tab: one at most. */
const palettes = new Map<number, OpenPalette>();

/** Tells the tab's page; hiding keefast's icons, its frames too. */
function tell(tab: number, message: TabMessage): Promise<void> {
  const options = message.type === "hide-fields" ? {} : { frameId: 0 };
  return browser.tabs
    .sendMessage(tab, message, options)
    .then(() => {})
    .catch(() => {});
}

/**
 * Opens the command palette over the page in `tab`; says whether it could. For a Card, `frame`
 * is the frame of another origin the user clicked keefast's icon in.
 */
async function openPalette(tab: number, mode: PaletteMode, frame?: ChosenFrame): Promise<boolean> {
  const page = await tabs.inspect(tab);
  if (!page) return false;
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  const token = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  const other = frame && frame.origin !== page.origin ? frame : undefined;
  palettes.set(tab, { token, mode, origin: page.origin, frame: other });
  const url = `${browser.runtime.getURL(`/${PALETTE_PAGE}`)}#${token}`;
  await tell(tab, { type: "open-palette", url });
  return true;
}

function closePalette(tab: number): Promise<void> {
  palettes.delete(tab);
  return tell(tab, { type: "close-palette" });
}

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
    case "show-on-site":
      await showOnSite(request.tab);
      return null;
  }
}

/** Shows keefast's icons in the tab's sign-in fields now, and on its site from now on. */
async function showOnSite(tab: number): Promise<void> {
  const { url } = await browser.tabs.get(tab);
  if (!url) return;
  const origin = new URL(url).origin;
  await hiddenSites.setValue((await hiddenSites.getValue()).filter((site) => site !== origin));
  if (!(await browser.permissions.contains(ALL_SITES))) return;
  await browser.scripting
    .executeScript({ target: { tabId: tab }, files: [FIELDS_SCRIPT.file] })
    .catch(() => {});
}

async function answerPalette(
  tab: number,
  palette: OpenPalette,
  request: PaletteRequest,
): Promise<PaletteAnswers[PaletteRequest["type"]]> {
  switch (request.type) {
    case "open": {
      const listed = palette.mode === "entries" ? await companion.entries(tab) : undefined;
      return {
        mode: palette.mode,
        site: new URL(palette.origin).host,
        entries: listed?.entries ?? [],
        frame: palette.frame && new URL(palette.frame.origin).host,
      };
    }
    case "cards":
      return (await companion.cards(tab, request.query)) ?? [];
    case "fill-card": {
      if (palette.mode !== "payment") return false;
      const filled = await companion.fillCard(tab, request.uuid, palette.frame);
      if (filled) await closePalette(tab);
      return filled;
    }
    case "search":
      return (await companion.search(tab, request.query)) ?? [];
    case "fill": {
      const filled = request.found
        ? await companion.fillPicked(tab, request.uuid, await addSiteOnPick.getValue())
        : await companion.fill(tab, request.uuid);
      if (filled) await closePalette(tab);
      return filled;
    }
    case "generate": {
      const password = await companion.generate(tab);
      if (password === undefined) throw new KeefastError("failed", "The page can't be filled");
      return password;
    }
    case "fill-generated": {
      const filled = await companion.fillGenerated(tab);
      if (filled) await closePalette(tab);
      return filled;
    }
    case "hide-site": {
      const sites = await hiddenSites.getValue();
      if (!sites.includes(palette.origin)) await hiddenSites.setValue([...sites, palette.origin]);
      await tell(tab, { type: "hide-fields" });
      await closePalette(tab);
      return null;
    }
    case "close":
      await closePalette(tab);
      return null;
  }
}

/**
 * What clicking keefast's icon in a field does: fills the site's only Entry, or opens the
 * palette to pick one, or to generate a new password; or, when keefast isn't ready, nothing,
 * for the field to say why. A payment field's always opens the palette, never filling a Card
 * the user didn't pick: `frame` is the one it was clicked in.
 */
async function fieldClick(tab: number, field: FieldKind, frame: ChosenFrame): Promise<FieldClickAnswer> {
  const status = await client.status().catch(() => "failed" as const);
  if (status !== "paired") return { kind: "unavailable", status };
  if (field === "payment") {
    const opened = await openPalette(tab, "payment", frame);
    return opened ? { kind: "palette" } : { kind: "unavailable", status: "failed" };
  }
  if (field !== "new-password" && (await companion.shortcut(tab)) === "filled") {
    return { kind: "filled" };
  }
  const opened = await openPalette(tab, field === "new-password" ? "generate" : "entries");
  return opened ? { kind: "palette" } : { kind: "unavailable", status: "failed" };
}

/**
 * Answers a page's script: `url` is the document it runs in, the page's or a frame's, and `top`
 * the page's.
 */
async function answerPage(
  message: PageMessage,
  tab: number,
  url: URL,
  top: string | undefined,
): Promise<PageAnswers[PageMessage["type"]]> {
  switch (message.type) {
    case "submitted": {
      // The browser says which site the login was submitted on, not the page.
      const page = { url: url.href, origin: url.origin, submitUrl: message.submitUrl };
      // When keefast is locked or not running, the login can't be saved: there's no one to ask.
      companion.submitted(tab, page, message.login).catch(() => {});
      return null;
    }
    case "fields-start": {
      // Hidden on the site, keefast is hidden in its frames too.
      const hidden = await hiddenSites.getValue();
      return !hidden.includes(url.origin) && (top === undefined || !hidden.includes(top));
    }
    case "field-click":
      return fieldClick(tab, message.field, { url: url.href, origin: url.origin });
    case "connect":
      try {
        await client.pair();
        return { ok: true, value: null };
      } catch (error) {
        return { ok: false, reason: error instanceof KeefastError ? error.reason : "failed" };
      }
  }
}

function isPageMessage(message: unknown): message is PageMessage {
  if (typeof message !== "object" || message === null) return false;
  const { type } = message as { type?: unknown };
  switch (type) {
    case "submitted": {
      const { login, submitUrl } = message as Partial<Extract<PageMessage, { type: "submitted" }>>;
      return (
        typeof login === "object" &&
        login !== null &&
        typeof login.username === "string" &&
        typeof login.password === "string" &&
        (login.current === undefined || typeof login.current === "string") &&
        (submitUrl === undefined || typeof submitUrl === "string")
      );
    }
    case "field-click": {
      const { field } = message as { field?: unknown };
      return field === "login" || field === "totp" || field === "new-password" || field === "payment";
    }
    case "fields-start":
    case "connect":
      return true;
    default:
      return false;
  }
}

/** Whether a page's frame may say this: only what keefast's icons in payment fields say. */
function fromFrame(message: PageMessage): boolean {
  return (
    message.type === "fields-start" ||
    message.type === "connect" ||
    (message.type === "field-click" && message.field === "payment")
  );
}

function isPaletteMessage(message: unknown): message is PaletteMessage {
  if (typeof message !== "object" || message === null) return false;
  const { type, token, request } = message as Partial<PaletteMessage>;
  return (
    type === "palette" &&
    typeof token === "string" &&
    typeof request === "object" &&
    request !== null
  );
}

/** Answers with `value`, or the reason it failed. */
function respondWith(promise: Promise<unknown>, respond: (answer: Answer<unknown>) => void) {
  promise.then(
    (value) => respond({ ok: true, value }),
    (error: unknown) =>
      respond({ ok: false, reason: error instanceof KeefastError ? error.reason : "failed" }),
  );
}

function onMessage(
  message: unknown,
  sender: MessageSender,
  respond: (answer: unknown) => void,
): boolean {
  if (sender.id !== browser.runtime.id) return false;
  const tab = sender.tab?.id;
  if (tab === undefined) {
    // The popup.
    respondWith(answer(message as Request), respond);
    return true;
  }
  if (sender.url?.startsWith(browser.runtime.getURL(`/${PALETTE_PAGE}`))) {
    // A palette answers only for the one keefast opened in its tab.
    const palette = palettes.get(tab);
    if (!isPaletteMessage(message) || palette?.token !== message.token) return false;
    respondWith(answerPalette(tab, palette, message.request), respond);
    return true;
  }
  if (!sender.url || !isPageMessage(message)) return false;
  // In frames, only keefast's icons in payment fields run.
  if (sender.frameId !== 0 && !fromFrame(message)) return false;
  const url = new URL(sender.url);
  if (url.protocol !== "https:" && url.protocol !== "http:") return false;
  const top = sender.tab?.url ? new URL(sender.tab.url).origin : undefined;
  answerPage(message, tab, url, top).then(respond, () => respond(null));
  return true;
}

/**
 * Registers the script that shows keefast's icons in sign-in fields while the extension may
 * run in every page, and unregisters it when it may not.
 */
async function syncFieldsScript(): Promise<void> {
  const granted = await browser.permissions.contains(ALL_SITES);
  const registered = await browser.scripting.getRegisteredContentScripts({
    ids: [FIELDS_SCRIPT.id],
  });
  const script = {
    id: FIELDS_SCRIPT.id,
    // Relative to the extension root, as the manifest lists content scripts.
    js: [FIELDS_SCRIPT.file.slice(1)],
    matches: ALL_SITES.origins,
    // Payment fields are often in a payment provider's frame (ADR 0010).
    allFrames: true,
    runAt: "document_idle" as const,
    persistAcrossSessions: true,
  };
  if (granted && registered.length === 0) {
    await browser.scripting.registerContentScripts([script]);
  } else if (granted && registered[0]?.allFrames !== true) {
    // Registered by a version that showed the icons in pages alone.
    await browser.scripting.updateContentScripts([script]);
  } else if (!granted && registered.length > 0) {
    await browser.scripting.unregisterContentScripts({ ids: [FIELDS_SCRIPT.id] });
  }
}

export default defineBackground(() => {
  // Pages' scripts never read the extension's storage: Pairings live there.
  void browser.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });

  browser.runtime.onMessage.addListener(onMessage);

  browser.runtime.onInstalled.addListener(() => void syncFieldsScript());
  browser.runtime.onStartup.addListener(() => void syncFieldsScript());
  browser.permissions.onAdded.addListener(() => void syncFieldsScript());
  browser.permissions.onRemoved.addListener(() => void syncFieldsScript());

  browser.tabs.onRemoved.addListener((tab) => palettes.delete(tab));

  browser.commands.onCommand.addListener(async (command, tab) => {
    if (command !== "fill" || tab?.id === undefined) return;
    // A payment form gets the palette to pick a Card: a Card is never filled unpicked.
    const page = await tabs.inspect(tab.id);
    const payment = page?.fields.payment === true && !page.fields.login && !page.fields.totp;
    const outcome = payment
      ? "choose"
      : await companion.shortcut(tab.id).catch(() => "choose" as const);
    if (outcome === "filled") return;
    // The palette lists the Entries to choose from; the popup says what's wrong.
    const status = await client.status().catch(() => "failed" as const);
    if (status === "paired" && (await openPalette(tab.id, payment ? "payment" : "entries"))) {
      return;
    }
    await browser.action.openPopup().catch(() => {});
  });
});
