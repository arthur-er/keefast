// Finding a page's login form and filling it. This runs in the page, injected only when the
// user asks to fill (the popup or the shortcut): nothing is read or filled on page load.

/** What fills a login form: an Entry's username and password. */
export interface Credentials {
  username: string;
  password: string;
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

export const TEXT_TYPES = new Set(["text", "email", "tel"]);
const USERNAME_HINT = /user|login|e-?mail|identifier|account/i;

/** The page's login fields, or none if it has no login form the user could see. */
export function findLoginFields(document: Document): LoginFields | undefined {
  const inputs = Array.from(document.querySelectorAll("input")).filter(fillable);
  const passwords = inputs.filter((input) => input.type === "password");
  const password =
    passwords.find((input) => autocomplete(input) === "current-password") ??
    passwords.find((input) => autocomplete(input) !== "new-password") ??
    passwords[0];
  const textInputs = inputs.filter((input) => TEXT_TYPES.has(input.type));

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
function fillable(input: HTMLInputElement): boolean {
  return !input.disabled && !input.readOnly && shown(input);
}

/** Whether the user could see the field. */
export function shown(input: HTMLInputElement): boolean {
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
function setValue(input: HTMLInputElement, value: string): void {
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
