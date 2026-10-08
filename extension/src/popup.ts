// The popup: says whether keefast can fill sign-in forms, pairs the browser, and lists the
// page's Entries to fill. It never asks for a master password: keefast is unlocked in keefast
// itself (ADR 0001).
import type { EntrySummary } from "./companion";
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
  const entries = await ask({ type: "entries", tab });
  if (!entries.ok) return failed(entries.reason);
  if (entries.value === null) return message("keefast can't fill this page.");
  if (entries.value.length === 0) return message("No Entries for this site.");
  const list = element("ul");
  list.append(...entries.value.map((entry) => entryItem(tab, entry)));
  show(list);
  list.querySelector("button")?.focus();
}

function entryItem(tab: number, entry: EntrySummary): HTMLElement {
  const button = element("button", "", "entry");
  button.append(element("span", entry.name || entry.username, "title"));
  if (entry.username) button.append(element("span", entry.username, "muted"));
  button.addEventListener("click", async () => {
    const filled = await ask({ type: "fill", tab, uuid: entry.uuid });
    if (filled.ok && filled.value) {
      window.close();
    } else {
      message(
        "Nothing was filled.",
        "This page has no login form, or it changed. Open the popup again.",
      );
    }
  });
  const item = element("li");
  item.append(button);
  return item;
}

void start();
