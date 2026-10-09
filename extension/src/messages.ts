// What the popup asks the service worker, and its answers; and what pages tell it.
import type { SubmittedLogin } from "./capture";
import type { PageEntries } from "./companion";
import type { FailureReason, Status } from "./keefast";

export type Request =
  | { type: "status" }
  | { type: "pair" }
  | { type: "entries"; tab: number }
  | { type: "fill"; tab: number; uuid: string }
  | { type: "generate"; tab: number }
  | { type: "fill-generated"; tab: number };

export type Answer<T> = { ok: true; value: T } | { ok: false; reason: FailureReason };

export interface Answers {
  status: Status;
  pair: null;
  /** None when the extension can't fill the page. */
  entries: PageEntries | null;
  fill: boolean;
  /** The password keefast's generator made; none when the extension can't fill the page. */
  generate: string | null;
  "fill-generated": boolean;
}

/** What the script in a page the user filled tells the service worker. */
export interface PageMessage {
  type: "submitted";
  login: SubmittedLogin;
  /** Where the login's form submitted to, when it was in a form. */
  submitUrl?: string;
}
