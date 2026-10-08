// Reading the login a user submits in a page, to offer keefast to save it. This runs only in
// pages the user has already asked the extension to fill.
import { TEXT_TYPES, autocomplete, precedes, shown } from "./forms";

/** What a scripted button that submits a login says. */
const SUBMIT_LABEL =
  /log\s*-?\s*[io]n|sign\s*-?\s*(in|up|on)|register|create|continue|next|submit|save|change|update|confirm|done/i;
/** What buttons that look like they submit, but don't, say. */
const NOT_SUBMIT_LABEL = /show|hide|reveal|forgot|reset|cancel|back|close/i;

/** A login the user submitted: a sign-in, a sign-up, or a password change. */
export interface SubmittedLogin {
  /** Empty when the form had no username, as password change forms often don't. */
  username: string;
  /** The password to keep: the new one, on a sign-up or a password change. */
  password: string;
  /** On a password change, the password it replaces. */
  current?: string;
}

/**
 * The login submitted with `scope`: a form, or the page for fields outside any form. None
 * when no password was typed, or a new password was typed differently the second time.
 */
export function submittedLogin(scope: HTMLFormElement | Document): SubmittedLogin | undefined {
  const inputs =
    scope instanceof HTMLFormElement
      ? Array.from(scope.elements).filter((element) => element instanceof HTMLInputElement)
      : Array.from(scope.querySelectorAll("input")).filter((input) => !input.form);
  const passwords = inputs.filter((input) => input.type === "password" && input.value !== "");
  const first = passwords[0];
  const typed = first && roles(passwords);
  if (!typed) {
    return undefined;
  }
  const username = usernameBefore(
    inputs.filter((input) => TEXT_TYPES.has(input.type) && input.value !== ""),
    first,
  );
  const { password, current } = typed;
  return current === undefined || current === password
    ? { username, password }
    : { username, password, current };
}

/**
 * The password to keep and the current one it replaces, from the typed password fields:
 * by their autocomplete marks, or else by their order (current, new, confirmation). None
 * when the new password and its confirmation differ, or two unmarked passwords do.
 */
function roles(
  passwords: HTMLInputElement[],
): { password: string; current?: string } | undefined {
  const marked = passwords.some((input) => {
    const mark = autocomplete(input);
    return mark === "current-password" || mark === "new-password";
  });
  let current: HTMLInputElement | undefined;
  let fresh: HTMLInputElement[];
  if (marked) {
    // Unmarked fields go by their place: before every new password, one is the current.
    const news = passwords.filter((input) => autocomplete(input) === "new-password");
    current = passwords.find(
      (input) =>
        autocomplete(input) === "current-password" ||
        (autocomplete(input) !== "new-password" && news.every((news) => precedes(input, news))),
    );
    fresh = passwords.filter((input) => input !== current);
  } else if (passwords.length === 1) {
    fresh = passwords;
  } else if (passwords.length === 2) {
    // A sign-up's password typed twice. Typed differently, it may be a mistyped sign-up or a
    // change without confirmation: neither is safe to save.
    fresh = passwords;
  } else {
    [current, ...fresh] = passwords;
  }
  const password = fresh[0]?.value ?? current?.value;
  if (password === undefined || fresh.some((input) => input.value !== password)) {
    return undefined;
  }
  return { password, current: fresh.length > 0 ? current?.value : undefined };
}

/**
 * The username typed with the password: the field marked as the username, even hidden as on
 * password change forms, or else the last text field the user could see before the password.
 */
function usernameBefore(textInputs: HTMLInputElement[], password: HTMLInputElement): string {
  const marked = textInputs.find((input) => autocomplete(input) === "username");
  const before = textInputs.filter((input) => shown(input) && precedes(input, password));
  return (marked ?? before[before.length - 1])?.value ?? "";
}

/**
 * Whether clicking `target` submits a login: a form's submit button, or a button a page
 * handles in script that says it submits. Not, say, the button showing the password.
 */
export function submitsLogin(target: Element): boolean {
  const button = target.closest("button, input, [role=button]");
  if (button instanceof HTMLButtonElement || button instanceof HTMLInputElement) {
    if (button.type === "submit" || button.type === "image") return true;
    if (button.type !== "button") return false;
  }
  if (!button) return false;
  const label = [
    button.textContent,
    button instanceof HTMLInputElement ? button.value : "",
    button.getAttribute("aria-label"),
    button.getAttribute("title"),
  ].join(" ");
  return SUBMIT_LABEL.test(label) && !NOT_SUBMIT_LABEL.test(label);
}
