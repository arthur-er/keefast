// Finding a page's login form, 2FA field and new password fields, and filling them. This runs
// in the page, injected only when the user asks to fill (the popup or the shortcut): nothing is
// read or filled on page load. Payment forms are found and filled in ./payment.ts.
import { findPaymentFields, isPaymentMarked } from "./payment";

/** What fills a login form: an Entry's username and password, and its TOTP code. */
export interface Credentials {
  username: string;
  password: string;
  /** The current code of the Entry's TOTP, for a 2FA field. */
  totp?: string;
}

/** The smallest a field can be, in pixels each way, for the user to see it. */
const MIN_VISIBLE_SIZE = 4;

/** A page's login fields. A sign-in split over steps may have only one of them. */
export interface LoginFields {
  username?: HTMLInputElement;
  password?: HTMLInputElement;
  /** Where the form submits to, when it is in a form. */
  submitUrl?: string;
}

/** What a page has for the extension to fill. */
export interface PageFields {
  /** A login form, or one step of it: a username or a password field. */
  login: boolean;
  /** A field for a TOTP code, as a 2FA step of a sign-in has. */
  totp: boolean;
  /** Fields for a new password, as sign-up and password change forms have. */
  newPassword: boolean;
  /** A payment form's fields, which a Card fills. */
  payment: boolean;
}

export const TEXT_TYPES = new Set(["text", "email", "tel"]);
const USERNAME_HINT = /user|login|e-?mail|identifier|account/i;

const TOTP_TYPES = new Set(["text", "tel", "number"]);
/** What only a 2FA code field is labelled. */
const TOTP_HINT =
  /totp|\botp|one.?time|2fa|two.?factor|mfa|authenticat|verification.?code|security.?code/i;
/** What a 2FA code field may be labelled, among other fields as short and numeric. */
const CODE_HINT = /code|token|\bpin\b/i;
/** What other code fields are labelled. */
const NOT_TOTP_HINT =
  /promo|coupon|voucher|gift|discount|referral|invit|postal|post.?code|zip|country|area|captcha/i;
/** What a form asking for a code split over digit fields says, around the fields. */
const CODE_CONTEXT = /code|verif|2fa|two.?factor|authenticat|one.?time|\botp/i;
/** How many digits a TOTP code has, at fewest and at most. */
const TOTP_DIGITS = { min: 6, max: 8 };

/** What a new password's field, or its confirmation's, is labelled. */
const NEW_PASSWORD_HINT = /new|confirm|repeat|retype|again|sign.?up|regist|create/i;

/** Whether a page is a sign-in's 2FA step: it asks for a TOTP code alone. */
export function isTwoFactorStep({ login, totp }: PageFields): boolean {
  return totp && !login;
}

/** What the page has for the extension to fill. */
export function pageFields(document: Document): PageFields {
  return {
    login: findLoginFields(document) !== undefined,
    totp: findTotpFields(document).length > 0,
    newPassword: findNewPasswordFields(document).length > 0,
    payment: findPaymentFields(document).length > 0,
  };
}

/** The page's login fields, or none if it has no login form the user could see. */
export function findLoginFields(document: Document): LoginFields | undefined {
  const inputs = Array.from(document.querySelectorAll("input")).filter(fillable);
  const passwords = inputs.filter((input) => input.type === "password");
  const password =
    passwords.find((input) => autocomplete(input) === "current-password") ??
    passwords.find((input) => autocomplete(input) !== "new-password") ??
    passwords[0];
  const textInputs = inputs.filter(
    (input) => TEXT_TYPES.has(input.type) && !isTotpField(input) && !isPaymentMarked(input),
  );

  if (password) {
    // The username is in the password's form (or, outside forms, among the fields outside
    // forms), before the password.
    const before = textInputs.filter(
      (input) => input.form === password.form && precedes(input, password),
    );
    const username =
      before.find((input) => autocomplete(input) === "username") ?? before[before.length - 1];
    return withSubmitUrl({ username, password });
  }

  const username =
    textInputs.find((input) => autocomplete(input) === "username") ??
    textInputs.find((input) => input.type === "email") ??
    textInputs.find((input) => USERNAME_HINT.test(`${input.name} ${input.id}`));
  return username ? withSubmitUrl({ username }) : undefined;
}

/**
 * Fills the page's login form with an Entry's credentials, if the page is still at `origin`,
 * the origin the Entry was asked for: the page may have navigated meanwhile. Says whether it
 * filled anything.
 */
export function fillLogin(
  document: Document,
  origin: string,
  { username, password }: Credentials,
): boolean {
  if (document.location.origin !== origin) {
    return false;
  }
  const fields = findLoginFields(document);
  if (!fields) {
    return false;
  }
  if (fields.username) {
    setValue(fields.username, username);
  }
  if (fields.password) {
    setValue(fields.password, password);
  }
  return true;
}

/**
 * The page's field for a TOTP code: one field, or a field for each of the code's digits. None
 * when the page has none the user could see.
 */
export function findTotpFields(document: Document): HTMLInputElement[] {
  const inputs = Array.from(document.querySelectorAll("input")).filter(
    (input) => TOTP_TYPES.has(input.type) && fillable(input),
  );
  const field = inputs.find(isTotpField);
  return field ? [field] : digitFields(inputs);
}

/**
 * Fills a TOTP code into the page's 2FA field, if the page is still at `origin`, the origin
 * the code's Entry was asked for. Says whether it filled it.
 */
export function fillTotp(document: Document, origin: string, code: string): boolean {
  if (document.location.origin !== origin) return false;
  const fields = findTotpFields(document);
  if (fields.length === 0) return false;
  if (fields.length === 1) {
    setValue(fields[0]!, code);
  } else {
    if (fields.length !== code.length) return false;
    fields.forEach((field, index) => setValue(field, code[index]!));
  }
  return true;
}

/**
 * The page's fields for a new password and its confirmation: those marked as new passwords,
 * or else a form's password typed twice, after the current one on a password change. A lone
 * password field is a new password only when it is labelled one.
 */
export function findNewPasswordFields(document: Document): HTMLInputElement[] {
  const passwords = Array.from(document.querySelectorAll("input")).filter(
    (input) => input.type === "password" && fillable(input),
  );
  const marked = passwords.filter((input) => autocomplete(input) === "new-password");
  if (marked.length > 0) return marked;
  const forms = new Set(passwords.map((input) => input.form));
  for (const form of forms) {
    const fields = passwords.filter((input) => input.form === form);
    const unmarked = fields.filter((input) => autocomplete(input) !== "current-password");
    let fresh: HTMLInputElement[];
    if (unmarked.length < fields.length || fields.length === 2) {
      fresh = unmarked;
    } else if (fields.length > 2) {
      fresh = fields.slice(1);
    } else {
      fresh = fields.filter((input) => NEW_PASSWORD_HINT.test(labels(input)));
    }
    if (fresh.length > 0) return fresh;
  }
  return [];
}

/**
 * Fills a generated password into the page's new password fields, if the page is still at
 * `origin`, the origin it was made for. Says whether it filled them.
 */
export function fillNewPassword(document: Document, origin: string, password: string): boolean {
  if (document.location.origin !== origin) return false;
  const fields = findNewPasswordFields(document);
  fields.forEach((field) => setValue(field, password));
  return fields.length > 0;
}

/** Whether a field is for a 2FA code, by how it is marked or labelled. */
function isTotpField(input: HTMLInputElement): boolean {
  if (autocomplete(input) === "one-time-code") return true;
  if (!TOTP_TYPES.has(input.type)) return false;
  const label = labels(input);
  if (NOT_TOTP_HINT.test(label)) return false;
  if (TOTP_HINT.test(label)) return true;
  const short = input.maxLength >= TOTP_DIGITS.min && input.maxLength <= TOTP_DIGITS.max;
  return CODE_HINT.test(label) && (short || input.inputMode === "numeric");
}

/**
 * The first run of single-character fields side by side, one for each digit of a TOTP code,
 * in a form that says it asks for a code: split card numbers and PINs look alike.
 */
function digitFields(inputs: HTMLInputElement[]): HTMLInputElement[] {
  let run: HTMLInputElement[] = [];
  for (const input of [...inputs, undefined]) {
    if (input?.maxLength === 1 && (run.length === 0 || besideEachOther(run[0]!, input))) {
      run.push(input);
      continue;
    }
    if (run.length >= TOTP_DIGITS.min && run.length <= TOTP_DIGITS.max && asksForCode(run[0]!)) {
      return run;
    }
    run = input?.maxLength === 1 ? [input] : [];
  }
  return [];
}

/** Whether the form of a digit field, or else its page, says it asks for a code. */
function asksForCode(input: HTMLInputElement): boolean {
  const scope = input.form ?? input.ownerDocument.body;
  const text = scope.textContent ?? "";
  return CODE_CONTEXT.test(`${text} ${labels(input)}`) && !NOT_TOTP_HINT.test(text);
}

/** Whether two fields are in the same box, alone in boxes of their own or not. */
function besideEachOther(a: HTMLInputElement, b: HTMLInputElement): boolean {
  const box = (input: HTMLInputElement) => {
    const parent = input.parentElement;
    return parent?.querySelectorAll("input").length === 1 ? parent.parentElement : parent;
  };
  return box(a) === box(b);
}

/** What a field is named and labelled, for telling what it is for. */
export function labels(input: HTMLInputElement | HTMLSelectElement): string {
  return [
    input.name,
    input.id,
    input.getAttribute("placeholder"),
    input.getAttribute("aria-label"),
    ...Array.from(input.labels ?? [], (label) => label.textContent),
  ].join(" ");
}

function withSubmitUrl(fields: LoginFields): LoginFields {
  const form = (fields.password ?? fields.username)?.form;
  return form ? { ...fields, submitUrl: form.action } : fields;
}

export function autocomplete(input: HTMLInputElement): string {
  return input.autocomplete.toLowerCase();
}

export function precedes(a: Node, b: Node): boolean {
  return (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
}

/** Whether the user could see and type into the field: a page may hide fields to harvest. */
export function fillable(input: HTMLInputElement): boolean {
  return !input.disabled && !input.readOnly && shown(input);
}

/** Whether the user could see the field. */
export function shown(input: HTMLInputElement | HTMLSelectElement): boolean {
  if (input.hidden) {
    return false;
  }
  const view = input.ownerDocument.defaultView;
  for (let element: Element | null = input; element; element = element.parentElement) {
    const style = view?.getComputedStyle(element);
    if (style?.display === "none" || style?.opacity === "0") {
      return false;
    }
  }
  if (view?.getComputedStyle(input).visibility === "hidden") {
    return false;
  }
  // A shown field has a box, unless the page isn't laid out (as in tests).
  const box = input.getClientRects()[0];
  if (!box) {
    return true;
  }
  const scrollX = view?.scrollX ?? 0;
  const scrollY = view?.scrollY ?? 0;
  return (
    box.width >= MIN_VISIBLE_SIZE &&
    box.height >= MIN_VISIBLE_SIZE &&
    box.right + scrollX > 0 &&
    box.bottom + scrollY > 0
  );
}

/** Sets a field's value as typing would, so the page's scripts see the change. */
export function setValue(input: HTMLInputElement, value: string): void {
  input.focus();
  // Through the prototype's setter: frameworks such as React shadow `value` on the element.
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (setter) {
    setter.call(input, value);
  } else {
    input.value = value;
  }
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}
