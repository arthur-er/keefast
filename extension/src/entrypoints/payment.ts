// Injected into a page and its frames only when the user asks to fill a Card, from the command
// palette (ADR 0010): it says what payment fields its document has, and fills them. It keeps
// nothing, and tells the extension nothing on its own.
import { defineUnlistedScript } from "wxt/utils/define-unlisted-script";
import { type CardValues, type PaymentToken, fillPayment, paymentTokens } from "../payment";

/** A document's payment fields, as the service worker sees them. */
export interface PaymentFields {
  url: string;
  origin: string;
  tokens: PaymentToken[];
}

export interface PaymentApi {
  /** What the document's payment fields take; none for a page the extension doesn't fill. */
  inspect(): PaymentFields | undefined;
  /** Fills the payment fields, if the document is still at `origin`; says whether it did. */
  fill(origin: string, values: CardValues): boolean;
}

declare global {
  // In the extension's isolated world: the page's own scripts can't see it.
  var keefastPayment: PaymentApi | undefined;
}

const paymentApi: PaymentApi = {
  inspect() {
    if (location.protocol !== "https:" && location.protocol !== "http:") return undefined;
    return { url: location.href, origin: location.origin, tokens: paymentTokens(document) };
  },
  fill(origin, values) {
    return fillPayment(document, origin, values);
  },
};

export default defineUnlistedScript(() => {
  globalThis.keefastPayment = paymentApi;
});
