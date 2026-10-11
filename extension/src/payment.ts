// Finding a payment form's fields, and filling them from a Card (ADR 0010). Fields are found by
// their `autocomplete` tokens first, then by their names and labels, but only in a document with
// a card number field, so a sign-up's name field isn't taken for a cardholder's. The security code
// is never guessed: it goes only into a field the page marked `cc-csc`. Like forms.ts, this runs
// in the page only when the user asks to fill it, or to show keefast's icons.
import { fillable, labels, setValue, shown } from "./forms";

/** The `autocomplete` tokens of a payment form's fields that a Card fills. */
export const PAYMENT_TOKENS = [
  "cc-number",
  "cc-name",
  "cc-exp",
  "cc-exp-month",
  "cc-exp-year",
  "cc-csc",
] as const;

export type PaymentToken = (typeof PAYMENT_TOKENS)[number];

/** A Card's values, by the token of the field each goes into, as keefast gives them. */
export type CardValues = Partial<Record<PaymentToken, string>>;

/** A payment form's field: what it takes, and whether the page said so or keefast guessed. */
export interface PaymentField {
  element: HTMLInputElement | HTMLSelectElement;
  token: PaymentToken;
  /** The page marked it with the token, in its `autocomplete` attribute. */
  marked: boolean;
}

/** Types of inputs a payment field may be. */
const PAYMENT_TYPES = new Set(["text", "tel", "number", "month", "search", ""]);

/** What fields are named and labelled, for those the page didn't mark, best guess first. */
const HINTS: [PaymentToken, RegExp][] = [
  ["cc-name", /card.?holder|name.?on.?(the.?)?card|cc.?name|holder.?name|card.?name|karteninhaber|titular/i],
  [
    "cc-number",
    /card.?num|cc.?num|cardnumber|card.?no\b|credit.?card|debit.?card|\bpan\b|kartennummer|num[eé]ro.?(de.?)?(la.?)?(carte|tarjeta)/i,
  ],
  ["cc-exp-month", /(exp|valid).{0,12}(month|mon\b|\bmm\b)|cc.?month|card.?month/i],
  ["cc-exp-year", /(exp|valid).{0,12}(year|yr\b|\byy)|cc.?year|card.?year/i],
  ["cc-exp", /expir|exp.?date|valid.?(thru|through|until)|\bmm\s*\/\s*yy|\bexp\b|ablauf|caducidad/i],
];

/** The field-name tokens of an `autocomplete` attribute: `billing cc-number` has `cc-number`. */
function tokens(element: HTMLInputElement | HTMLSelectElement): string[] {
  return (element.getAttribute("autocomplete") ?? "").toLowerCase().split(/\s+/).filter(Boolean);
}

/** The payment token a field is marked with, if it is marked with one. */
function markedToken(element: HTMLInputElement | HTMLSelectElement): PaymentToken | undefined {
  return tokens(element).find((token): token is PaymentToken =>
    (PAYMENT_TOKENS as readonly string[]).includes(token),
  );
}

/** Whether the page marked a field as a payment form's, so it is no login's. */
export function isPaymentMarked(element: HTMLInputElement | HTMLSelectElement): boolean {
  return markedToken(element) !== undefined;
}

/** Whether the page marked a field as being for something else, such as `email` or `username`. */
function markedOtherwise(element: HTMLInputElement | HTMLSelectElement): boolean {
  return tokens(element).some((token) => token !== "on" && token !== "off" && !token.startsWith("section-"));
}

/** Whether the user could see and change the field. */
function usable(element: HTMLInputElement | HTMLSelectElement): boolean {
  return element instanceof element.ownerDocument.defaultView!.HTMLSelectElement
    ? !element.disabled && shown(element)
    : fillable(element as HTMLInputElement);
}

/**
 * The payment form's fields in a document the user could see and fill, in the page's order.
 * Fields keefast guessed count only beside a card number field: on its own, a name or a date
 * isn't a payment form's. A frame of a payment provider may hold one field alone, so a field the
 * page marked counts without a number. With `guessesAlone`, for filling a Card the user picked
 * for the page, guesses count without a number too: the number may be in a provider's frame.
 */
export function findPaymentFields(
  document: Document,
  { guessesAlone = false } = {},
): PaymentField[] {
  const view = document.defaultView;
  if (!view) return [];
  const elements = Array.from(
    document.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input, select"),
  ).filter(
    (element) =>
      (element instanceof view.HTMLSelectElement || PAYMENT_TYPES.has(element.type)) &&
      usable(element),
  );
  const marked: PaymentField[] = [];
  const guessed: PaymentField[] = [];
  for (const element of elements) {
    const token = markedToken(element);
    if (token) {
      marked.push({ element, token, marked: true });
      continue;
    }
    if (markedOtherwise(element)) continue;
    const text = labels(element);
    // Never guessed: the security code, and a select for anything but a month or a year.
    const hint = HINTS.find(
      ([token, pattern]) =>
        pattern.test(text) &&
        (!(element instanceof view.HTMLSelectElement) ||
          token === "cc-exp-month" ||
          token === "cc-exp-year"),
    );
    if (hint) guessed.push({ element, token: hint[0], marked: false });
  }
  const hasNumber = [...marked, ...guessed].some(({ token }) => token === "cc-number");
  const found = hasNumber || guessesAlone ? [...marked, ...guessed] : marked;
  return found.sort((a, b) =>
    a.element.compareDocumentPosition(b.element) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
  );
}

/**
 * The tokens of the payment fields in a document, each once, as a frame tells the extension when
 * the user has picked a Card for the page: guesses count without a number.
 */
export function paymentTokens(document: Document): PaymentToken[] {
  return [
    ...new Set(findPaymentFields(document, { guessesAlone: true }).map(({ token }) => token)),
  ];
}

/** A card's expiry, its year in four digits. */
export interface Expiry {
  month: number;
  year: number;
}

/**
 * The expiry a Card's values give: whole (`08/31`, `08/2031`, `0831`), as `cc-exp` holds it and
 * as {@link valuesFor} gives each expiry field a frame has, or else from a month and a year apart.
 */
export function expiryOf(values: CardValues): Expiry | undefined {
  const fullYear = (year: number) => (year < 100 ? 2000 + year : year);
  const valid = (month: number, year: number) =>
    month >= 1 && month <= 12 && year > 0 ? { month, year: fullYear(year) } : undefined;
  const whole = (exp: string) => {
    const parts =
      exp.match(/^(\d{1,2})\s*[/\-. ]\s*(\d{2}|\d{4})$/) ?? exp.match(/^(\d{2})(\d{2})$/);
    if (parts) return valid(Number(parts[1]), Number(parts[2]));
    // As `<input type="month">` writes it.
    const iso = exp.match(/^(\d{4})-(\d{2})$/);
    return iso ? valid(Number(iso[2]), Number(iso[1])) : undefined;
  };
  for (const token of ["cc-exp", "cc-exp-month", "cc-exp-year"] as const) {
    const expiry = values[token] && whole(values[token].trim());
    if (expiry) return expiry;
  }
  if (values["cc-exp-month"] && values["cc-exp-year"]) {
    return valid(Number(values["cc-exp-month"]), Number(values["cc-exp-year"]));
  }
  return undefined;
}

/**
 * The values a frame with fields of these tokens takes from a Card: each only if the frame has
 * a field for it, the expiry in whatever form the frame's fields take it. The security code only
 * for a frame with a field marked `cc-csc`.
 */
export function valuesFor(frameTokens: readonly PaymentToken[], card: CardValues): CardValues {
  const values: CardValues = {};
  const expiry = expiryOf(card);
  for (const token of frameTokens) {
    if (isExpiry(token)) {
      // Whole, whichever part the field takes: it writes it as it wants it.
      if (expiry) values[token] = `${pad(expiry.month)}/${expiry.year}`;
    } else if (card[token] !== undefined) {
      values[token] = card[token];
    }
  }
  return values;
}

const isExpiry = (token: PaymentToken) =>
  token === "cc-exp" || token === "cc-exp-month" || token === "cc-exp-year";

const pad = (number: number) => String(number).padStart(2, "0");

/** A field's hint at how it wants the expiry: its placeholder, or else its label. */
function formatHint(input: HTMLInputElement): string {
  return (input.getAttribute("placeholder") || input.getAttribute("aria-label") || "").toLowerCase();
}

/** The expiry as an input taking month and year together wants it. */
function expiryText(input: HTMLInputElement, { month, year }: Expiry): string {
  if (input.type === "month") return `${year}-${pad(month)}`;
  const hint = formatHint(input);
  const short = String(year % 100).padStart(2, "0");
  const shape = hint.match(/(mm|m)(\s*[/\-. ]?\s*)(yyyy|aaaa|jjjj|yy|aa|jj)/);
  if (shape) return `${pad(month)}${shape[2]}${shape[3]!.length === 4 ? year : short}`;
  if (input.maxLength === 4) return `${pad(month)}${short}`;
  return `${pad(month)}/${short}`;
}

/** The year as an input taking it alone wants it: two digits when it says so. */
function yearText(input: HTMLInputElement, year: number): string {
  const hint = formatHint(input);
  const two = input.maxLength === 2 || /^\s*(yy|aa|jj)\s*$/.test(hint);
  return two ? String(year % 100).padStart(2, "0") : String(year);
}

/** Picks the option whose value or text is the number, as `08`, `8`, `2031` or `31` might be. */
function selectNumber(select: HTMLSelectElement, numbers: number[], names: string[] = []): boolean {
  const options = Array.from(select.options).filter((option) => !option.disabled);
  const numberIn = (text: string) => {
    const digits = text.trim().match(/^\d+/);
    return digits ? Number(digits[0]) : undefined;
  };
  const option =
    options.find((option) => numbers.includes(numberIn(option.value) ?? NaN)) ??
    options.find((option) => numbers.includes(numberIn(option.text) ?? NaN)) ??
    options.find((option) =>
      names.some((name) => option.text.trim().toLowerCase().startsWith(name)),
    );
  if (!option) return false;
  select.focus();
  select.value = option.value;
  select.dispatchEvent(new Event("input", { bubbles: true }));
  select.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/** Fills one field with its value; says whether it did. */
function fillField({ element, token }: PaymentField, value: string): boolean {
  const view = element.ownerDocument.defaultView!;
  const expiry = isExpiry(token) ? expiryOf({ "cc-exp": value }) : undefined;
  if (element instanceof view.HTMLSelectElement) {
    if (!expiry) return false;
    if (token === "cc-exp-month") {
      return selectNumber(element, [expiry.month], [MONTHS[expiry.month - 1]!]);
    }
    if (token === "cc-exp-year") return selectNumber(element, [expiry.year, expiry.year % 100]);
    return false;
  }
  const input = element as HTMLInputElement;
  switch (token) {
    case "cc-number":
      // The page spaces it as it likes; some fields hold no more than the digits.
      setValue(input, value.replace(/\D/g, ""));
      return true;
    case "cc-exp":
      if (!expiry) return false;
      setValue(input, expiryText(input, expiry));
      return true;
    case "cc-exp-month":
      if (!expiry) return false;
      setValue(input, pad(expiry.month));
      return true;
    case "cc-exp-year":
      if (!expiry) return false;
      setValue(input, yearText(input, expiry.year));
      return true;
    case "cc-name":
    case "cc-csc":
      setValue(input, value);
      return true;
  }
}

/**
 * Fills a document's payment fields with the values of a Card the user picked for the page, if
 * it is still at `origin`, the origin they were given for: it may have navigated meanwhile. Each
 * value goes into the first field taking it; guesses count without a number in this document,
 * as the number may be in a provider's frame. The security code goes only into a field marked
 * `cc-csc`. Says whether it filled anything.
 */
export function fillPayment(document: Document, origin: string, values: CardValues): boolean {
  if (document.location.origin !== origin) return false;
  const done = new Set<PaymentToken>();
  let filled = false;
  for (const field of findPaymentFields(document, { guessesAlone: true })) {
    if (done.has(field.token)) continue;
    // Guessed fields never get the security code, whatever they are called.
    if (field.token === "cc-csc" && (!field.marked || markedToken(field.element) !== "cc-csc")) {
      continue;
    }
    const value = valuesFor([field.token], values)[field.token];
    if (value === undefined || value === "") continue;
    if (fillField(field, value)) {
      done.add(field.token);
      filled = true;
    }
  }
  return filled;
}
