// Frames the command palette over a page, when the service worker says. The palette is an
// extension page in a frame, in a closed shadow root: the page sees neither what it lists nor
// what the user types there (ADR 0007).
import { browser } from "wxt/browser";
import type { TabMessage } from "./messages";

declare global {
  // In the extension's isolated world: the page's own scripts can't see it. Shared by the
  // page script and the fields script, which are bundled apart.
  var keefastPaletteHost: { hideFields: (() => void)[] } | undefined;
}

/** The palette's frame, as long as it is open, and what had focus before it opened. */
let open: { host: HTMLElement; previous: Element | null } | undefined;

function openPalette(url: string): void {
  // Only the extension's own palette.
  if (!url.startsWith(browser.runtime.getURL("/palette.html"))) return;
  const previous = open?.previous ?? document.activeElement;
  closePalette({ restoreFocus: false });
  const host = document.createElement("keefast-palette");
  host.style.cssText =
    "all: initial !important; position: fixed !important; inset: 0 !important; " +
    "z-index: 2147483647 !important; display: block !important;";
  const shadow = host.attachShadow({ mode: "closed" });
  const frame = document.createElement("iframe");
  frame.src = url;
  frame.title = "keefast";
  // The palette draws its own backdrop. A frame is see-through only with the same colour
  // scheme as the page around it, so both are left at their normal one.
  frame.style.cssText =
    "border: 0; width: 100%; height: 100%; display: block; background: transparent; " +
    "color-scheme: normal;";
  frame.addEventListener("load", () => frame.focus());
  shadow.append(frame);
  document.documentElement.append(host);
  open = { host, previous };
}

function closePalette({ restoreFocus = true } = {}): void {
  if (!open) return;
  const { host, previous } = open;
  open = undefined;
  host.remove();
  if (restoreFocus && previous instanceof HTMLElement) previous.focus({ preventScroll: true });
}

/**
 * Opens and closes the palette as the service worker says, once per page, and calls
 * `onHideFields` when the user hides keefast on the site.
 */
export function hostPalette(onHideFields?: () => void): void {
  const started = globalThis.keefastPaletteHost;
  const host = started ?? { hideFields: [] };
  if (onHideFields) host.hideFields.push(onHideFields);
  if (started) return;
  globalThis.keefastPaletteHost = host;
  browser.runtime.onMessage.addListener((message: TabMessage) => {
    switch (message?.type) {
      case "open-palette":
        openPalette(message.url);
        break;
      case "close-palette":
        closePalette();
        break;
      case "hide-fields":
        host.hideFields.forEach((listener) => listener());
        break;
    }
  });
}
