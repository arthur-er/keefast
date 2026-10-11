// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it } from "vitest";
import {
  type CardValues,
  expiryOf,
  fillPayment,
  findPaymentFields,
  paymentTokens,
  valuesFor,
} from "../src/payment";
import { findLoginFields, pageFields } from "../src/forms";

// Relative to the extension, where the tests run: jsdom gives modules no file URL.
const CHECKOUT = readFileSync("test/fixtures/checkout.html", "utf8");

function page(html: string): void {
  document.body.innerHTML = html;
}

/** The checkout fixture, its styles and all. */
function checkout(): void {
  document.documentElement.innerHTML = new DOMParser().parseFromString(
    CHECKOUT,
    "text/html",
  ).documentElement.innerHTML;
}

const byId = <T extends HTMLElement = HTMLInputElement>(id: string) =>
  document.getElementById(id) as T;

const visa: CardValues = {
  "cc-number": "4111 1111 1111 1111",
  "cc-name": "Ada Lovelace",
  "cc-exp": "08/31",
  "cc-csc": "737",
};

/** jsdom's origin for the pages here. */
const ORIGIN = "http://localhost:3000";

beforeEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

describe("findPaymentFields", () => {
  it("finds a checkout's fields, by autocomplete first and by label second", () => {
    checkout();

    const found = findPaymentFields(document).map(({ element, token, marked }) => [
      element.id,
      token,
      marked,
    ]);

    expect(found).toEqual([
      ["holder", "cc-name", false],
      ["number", "cc-number", true],
      ["month", "cc-exp-month", true],
      ["year", "cc-exp-year", false],
      ["cvc", "cc-csc", true],
    ]);
    // Not the email, the promo code, nor the hidden fields a page may harvest with.
    expect(pageFields(document).payment).toBe(true);
  });

  it("guesses fields only beside a card number", () => {
    page(`
      <form>
        <input id="name" name="cardholder">
        <input id="exp" name="expiry" placeholder="MM/YY">
      </form>`);

    expect(findPaymentFields(document)).toEqual([]);
    expect(pageFields(document).payment).toBe(false);
  });

  it("guesses alone when filling a Card picked for the page, its number in another frame", () => {
    page(`<input id="name" name="name-on-card"><input id="email" name="email">`);

    expect(findPaymentFields(document)).toEqual([]);
    expect(paymentTokens(document)).toEqual(["cc-name"]);
    fillPayment(document, ORIGIN, visa);
    expect(byId("name").value).toBe("Ada Lovelace");
    expect(byId("email").value).toBe("");
  });

  it("takes a lone field the page marked, as a payment provider's frame has", () => {
    page(`<input id="cvc" autocomplete="cc-csc">`);

    expect(paymentTokens(document)).toEqual(["cc-csc"]);
  });

  it("never guesses the security code", () => {
    page(`
      <input id="number" autocomplete="cc-number">
      <input id="cvv" name="cvv" placeholder="CVV">
      <input id="csc" name="security-code" aria-label="Security code">`);

    expect(paymentTokens(document)).toEqual(["cc-number"]);
  });

  it("leaves fields the page marked as something else", () => {
    page(`
      <input id="number" name="card-number">
      <input id="user" name="card-holder" autocomplete="username">
      <input id="pass" name="card-name" type="password">`);

    expect(findPaymentFields(document).map(({ element }) => element.id)).toEqual(["number"]);
  });

  it("reads tokens with a section and billing before them", () => {
    page(`<input id="n" autocomplete="section-pay billing cc-number">`);

    expect(findPaymentFields(document)[0]).toMatchObject({ token: "cc-number", marked: true });
  });

  it("keeps a cardholder's name from being taken for a login's username", () => {
    page(`
      <input id="email" type="email" autocomplete="email">
      <input id="holder" autocomplete="cc-name" name="account-holder">`);

    expect(findLoginFields(document)?.username).toBe(byId("email"));
  });
});

describe("fillPayment", () => {
  it("fills the checkout fixture, the CVC only into the field marked cc-csc", () => {
    checkout();

    expect(fillPayment(document, ORIGIN, visa)).toBe(true);

    expect(byId("holder").value).toBe("Ada Lovelace");
    expect(byId("number").value).toBe("4111111111111111");
    expect(byId<HTMLSelectElement>("month").value).toBe("8");
    expect(byId<HTMLSelectElement>("year").value).toBe("31");
    expect(byId("cvc").value).toBe("737");
    for (const id of ["email", "promo", "trap", "trap-cvc"]) expect(byId(id).value, id).toBe("");
  });

  it("fills what a frame was given for its fields, as the extension gives it", () => {
    checkout();

    fillPayment(document, ORIGIN, valuesFor(paymentTokens(document), visa));

    expect(byId<HTMLSelectElement>("month").value).toBe("8");
    expect(byId<HTMLSelectElement>("year").value).toBe("31");
    expect(byId("cvc").value).toBe("737");
  });

  it("never fills the CVV into a field that isn't marked cc-csc", () => {
    page(`
      <input id="number" autocomplete="cc-number">
      <input id="cvv" name="cvv" placeholder="CVV" maxlength="4">
      <input id="pin" name="pin" autocomplete="cc-number-pin">
      <input id="code" name="code" autocomplete="one-time-code">`);

    fillPayment(document, ORIGIN, visa);

    expect(byId("number").value).toBe("4111111111111111");
    for (const id of ["cvv", "pin", "code"]) expect(byId(id).value, id).toBe("");
  });

  it("fills nothing once the page is at another origin", () => {
    page(`<input id="number" autocomplete="cc-number">`);

    expect(fillPayment(document, "https://shop.test", visa)).toBe(false);
    expect(byId("number").value).toBe("");
  });

  it("writes a combined expiry as the field shows it", () => {
    const cases: [attributes: string, expected: string][] = [
      [``, "08/31"],
      [`placeholder="MM / YY"`, "08 / 31"],
      [`placeholder="MM/YYYY"`, "08/2031"],
      [`placeholder="mm-aa"`, "08-31"],
      [`maxlength="4"`, "0831"],
      [`type="month"`, "2031-08"],
    ];
    for (const [attributes, expected] of cases) {
      page(`<input id="number" autocomplete="cc-number"><input id="exp" autocomplete="cc-exp" ${attributes}>`);
      fillPayment(document, ORIGIN, visa);
      expect(byId("exp").value, attributes).toBe(expected);
    }
  });

  it("splits the expiry into a month and a year of two or four digits", () => {
    page(`
      <input id="number" autocomplete="cc-number">
      <input id="month" autocomplete="cc-exp-month">
      <input id="year" autocomplete="cc-exp-year" maxlength="2">`);
    fillPayment(document, ORIGIN, visa);
    expect([byId("month").value, byId("year").value]).toEqual(["08", "31"]);

    page(`
      <input id="number" autocomplete="cc-number">
      <input id="month" autocomplete="cc-exp-month">
      <input id="year" autocomplete="cc-exp-year" placeholder="YYYY">`);
    fillPayment(document, ORIGIN, { ...visa, "cc-exp": "08/2031" });
    expect([byId("month").value, byId("year").value]).toEqual(["08", "2031"]);
  });

  it("picks the month and the year in selects, by value, by number or by name", () => {
    page(`
      <input id="number" autocomplete="cc-number">
      <select id="month" autocomplete="cc-exp-month">
        <option value="">--</option><option value="jan">Jan</option><option value="aug">Aug</option>
      </select>
      <select id="year" autocomplete="cc-exp-year">
        <option value="">--</option><option value="2030">2030</option><option value="2031">2031</option>
      </select>`);

    fillPayment(document, ORIGIN, visa);

    expect(byId<HTMLSelectElement>("month").value).toBe("aug");
    expect(byId<HTMLSelectElement>("year").value).toBe("2031");
  });

  it("fills a combined expiry from a Plugin's month and year apart", () => {
    page(`<input id="number" autocomplete="cc-number"><input id="exp" autocomplete="cc-exp">`);

    fillPayment(document, ORIGIN, { "cc-number": "4111", "cc-exp-month": "8", "cc-exp-year": "2031" });

    expect(byId("exp").value).toBe("08/31");
  });

  it("tells the page's scripts, as typing would", () => {
    page(`<input id="number" autocomplete="cc-number">`);
    const seen: string[] = [];
    byId("number").addEventListener("input", () => seen.push("input"));
    byId("number").addEventListener("change", () => seen.push("change"));

    fillPayment(document, ORIGIN, visa);

    expect(seen).toEqual(["input", "change"]);
  });
});

describe("valuesFor", () => {
  it("gives a frame only what its fields take, and the CVV only to a cc-csc field", () => {
    expect(valuesFor(["cc-number"], visa)).toEqual({ "cc-number": "4111 1111 1111 1111" });
    expect(valuesFor(["cc-exp-month", "cc-exp-year"], visa)).toEqual({
      "cc-exp-month": "08/2031",
      "cc-exp-year": "08/2031",
    });
    expect(valuesFor(["cc-csc"], visa)).toEqual({ "cc-csc": "737" });
    expect(valuesFor(["cc-name"], {})).toEqual({});
  });
});

describe("expiryOf", () => {
  it("reads the shapes a Plugin may keep an expiry in", () => {
    expect(expiryOf({ "cc-exp": "08/31" })).toEqual({ month: 8, year: 2031 });
    expect(expiryOf({ "cc-exp": "8/2031" })).toEqual({ month: 8, year: 2031 });
    expect(expiryOf({ "cc-exp": "0831" })).toEqual({ month: 8, year: 2031 });
    expect(expiryOf({ "cc-exp": "13/31" })).toBeUndefined();
    expect(expiryOf({ "cc-exp-month": "12", "cc-exp-year": "29" })).toEqual({ month: 12, year: 2029 });
  });
});
