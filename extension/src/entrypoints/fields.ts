// Shows keefast's icon in the sign-in fields of every page, once the user turns it on in the
// popup (ADR 0007): the service worker registers this script then. It reads no Entries: the
// icon asks the service worker when clicked. It runs in pages' frames too, where it shows the
// icon only in payment fields, which payment providers often put in frames of their own
// (ADR 0010); the command palette opens over the page, never in a frame.
import { browser } from "wxt/browser";
import { defineUnlistedScript } from "wxt/utils/define-unlisted-script";
import { FieldIcons } from "../field-icons";
import type { PageAnswers, PageMessage, TabMessage } from "../messages";
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
  const inFrame = window !== window.top;
  const icons = new FieldIcons(
    document,
    {
      click: (field) => ask({ type: "field-click", field }),
      async connect() {
        const answer = await ask({ type: "connect" });
        return answer.ok ? { ok: true } : answer;
      },
    },
    { paymentOnly: inFrame },
  );
  globalThis.keefastFields = icons;
  icons.start();
  const hide = () => {
    icons.stop();
    if (globalThis.keefastFields === icons) globalThis.keefastFields = undefined;
  };
  if (inFrame) {
    // Hiding keefast on the site hides it in the site's frames too.
    browser.runtime.onMessage.addListener((message: TabMessage) => {
      if (message?.type === "hide-fields") hide();
    });
  } else {
    hostPalette(hide);
  }
});
