// Shows keefast's icon in the sign-in fields of every page, once the user turns it on in the
// popup (ADR 0007): the service worker registers this script then. It reads no Entries: the
// icon asks the service worker when clicked.
import { browser } from "wxt/browser";
import { defineUnlistedScript } from "wxt/utils/define-unlisted-script";
import { FieldIcons } from "../field-icons";
import type { PageAnswers, PageMessage } from "../messages";
import { hostPalette } from "../palette-host";

declare global {
  // In the extension's isolated world: the page's own scripts can't see it.
  var keefastFields: FieldIcons | undefined;
}

function ask<M extends PageMessage>(message: M): Promise<PageAnswers[M["type"]]> {
  return browser.runtime.sendMessage(message);
}

export default defineUnlistedScript(async () => {
  if (globalThis.keefastFields) return;
  if (location.protocol !== "https:" && location.protocol !== "http:") return;
  // The user may have hidden keefast on the site.
  if ((await ask({ type: "fields-start" }).catch(() => false)) !== true) return;
  const icons = new FieldIcons(document, {
    click: (field) => ask({ type: "field-click", field }),
    async connect() {
      const answer = await ask({ type: "connect" });
      return answer.ok ? { ok: true } : answer;
    },
  });
  globalThis.keefastFields = icons;
  icons.start();
  hostPalette(() => {
    icons.stop();
    if (globalThis.keefastFields === icons) globalThis.keefastFields = undefined;
  });
});
