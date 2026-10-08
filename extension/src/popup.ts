// The popup: says whether keefast can fill sign-in forms, pairs the browser, lists the page's
// Entries to fill, with their TOTP codes on a 2FA step, and fills new passwords from keefast's
// generator. It never asks for a master password: keefast is unlocked in keefast itself
// (ADR 0001).
import type { EntrySummary } from "./companion";
import type { PageFields } from "./forms";
import type { FailureReason } from "./keefast";
import type { Answer, Answers, Request } from "./messages";

const main = document.getElementById("main")!;

async function ask<R extends Request>(request: R): Promise<Answer<Answers[R["type"]]>> {
  return chrome.runtime.sendMessage(request);
}

function element(tag: string, text = "", className = ""): HTMLElement {
  const created = document.createElement(tag);
  created.textContent = text;
  if (className) created.className = className;
  return created;
}

function show(...children: Node[]): void {
  main.replaceChildren(...children);
}

function message(title: string, detail = ""): void {
  show(element("p", title, "title"), ...(detail ? [element("p", detail, "muted")] : []));
}

function failed(reason: FailureReason): void {
  switch (reason) {
    case "not-running":
      return message(
        "keefast is not running — start keefast and unlock it.",
        "Browser integration must be on in keefast's settings.",
      );
    case "locked":
      return message("keefast is locked — unlock it.", "Then open this popup again.");
    case "denied":
      return message("Turned down in keefast.");
    case "failed":
      return message("Something went wrong talking to keefast.", "Try again.");
  }
}

async function start(): Promise<void> {
  message("Connecting to keefast…");
  const status = await ask({ type: "status" });
  if (!status.ok) return failed(status.reason);
  switch (status.value) {
    case "not-running":
    case "locked":
      return failed(status.value);
    case "unpaired":
      return offerPairing();
    case "paired":
      return listEntries();
  }
}

function offerPairing(): void {
  const connect = element("button", "Connect", "primary");
  connect.addEventListener("click", async () => {
    message("Approve the Pairing in keefast.", "keefast asks you to name this browser.");
    const paired = await ask({ type: "pair" });
    if (!paired.ok) {
      failed(paired.reason);
      main.append(retry());
      return;
    }
    await listEntries();
  });
  show(
    element("p", "Connect this browser to keefast.", "title"),
    element(
      "p",
      "You approve the Pairing in keefast, which then fills sign-in forms when you ask.",
      "muted",
    ),
    connect,
  );
}

function retry(): HTMLElement {
  const button = element("button", "Try again", "primary");
  button.addEventListener("click", () => void start());
  return button;
}

async function activeTab(): Promise<number | undefined> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab?.id;
}

async function listEntries(): Promise<void> {
  const tab = await activeTab();
  if (tab === undefined) return message("keefast can't fill this page.");
  message("Asking keefast for this site's Entries…", "If keefast asks, allow this site there.");
  const answer = await ask({ type: "entries", tab });
  if (!answer.ok) return failed(answer.reason);
  if (answer.value === null) return message("keefast can't fill this page.");
  const { entries, fields } = answer.value;
  const twoFactor = fields.totp && !fields.login;
  if (entries.length === 0) {
    if (twoFactor) {
      message("No Entry for this site has TOTP.", "Set up TOTP on its Entry in keefast.");
    } else {
      message("No Entries for this site.", "Sign in or sign up, and keefast offers to save it.");
    }
  } else {
    const list = element("ul");
    list.append(...entries.map((entry) => entryItem(tab, entry, fields)));
    show(list);
  }
  // A 2FA step has no use for a new password; a sign-up has no use for much else.
  if (!twoFactor) main.append(generator(tab, fields));
  const first = fields.newPassword ? ".generator button" : "button";
  main.querySelector<HTMLElement>(first)?.focus();
}

function entryItem(tab: number, entry: EntrySummary, fields: PageFields): HTMLElement {
  const button = element("button", "", "entry");
  const title = element("span", "", "title");
  title.append(element("span", entry.name || entry.username, "name"));
  if (entry.hasTotp && fields.totp) title.append(element("span", "TOTP", "badge"));
  button.append(title);
  if (entry.username) button.append(element("span", entry.username, "muted"));
  if (entry.hasTotp && fields.totp) {
    button.title = fields.login ? "Fills the login and its TOTP code" : "Fills its TOTP code";
  }
  button.addEventListener("click", async () => {
    const filled = await ask({ type: "fill", tab, uuid: entry.uuid });
    if (filled.ok && filled.value) {
      window.close();
    } else if (!filled.ok) {
      failed(filled.reason);
    } else {
      message(
        "Nothing was filled.",
        fields.login
          ? "This page has no login form, or it changed. Open the popup again."
          : "This page has no 2FA field, or it changed. Open the popup again.",
      );
    }
  });
  const item = element("li");
  item.append(button);
  return item;
}

/**
 * The generator: makes a password with keefast's generator settings, shows it, and fills it
 * into the page's new password fields, when the page has them.
 */
function generator(tab: number, fields: PageFields): HTMLElement {
  const section = element("section", "", "generator");
  const style = fields.newPassword ? "primary" : "secondary";
  const generate = element("button", "Generate password", style);
  generate.addEventListener("click", () => void makePassword(tab, fields, section));
  section.append(generate);
  return section;
}

async function makePassword(tab: number, fields: PageFields, section: HTMLElement): Promise<void> {
  const generated = await ask({ type: "generate", tab });
  if (!generated.ok || generated.value === null) {
    const detail = generated.ok
      ? "keefast can't fill this page."
      : generated.reason === "denied"
        ? "keefast's generator settings can't make a password. Change them in keefast."
        : "Something went wrong talking to keefast. Try again.";
    section.replaceChildren(element("p", detail, "muted"));
    return;
  }
  const password = document.createElement("input");
  password.className = "password";
  password.readOnly = true;
  password.value = generated.value;
  password.setAttribute("aria-label", "Generated password");
  password.spellcheck = false;
  const again = element("button", "Another", "secondary");
  again.addEventListener("click", () => void makePassword(tab, fields, section));
  const actions = element("div", "", "actions");
  if (fields.newPassword) {
    const fill = element("button", "Fill", "primary");
    fill.addEventListener("click", async () => {
      const filled = await ask({ type: "fill-generated", tab });
      if (filled.ok && filled.value) {
        window.close();
      } else {
        section.replaceChildren(
          element("p", "Nothing was filled.", "title"),
          element(
            "p",
            "This page has no new password field, or it changed. Open the popup again.",
            "muted",
          ),
        );
      }
    });
    actions.append(fill);
  }
  actions.append(again);
  section.replaceChildren(password, actions);
  if (!fields.newPassword) {
    section.append(element("p", "This page has no new password field to fill.", "muted"));
  }
  (actions.querySelector("button") as HTMLElement).focus();
}

void start();
