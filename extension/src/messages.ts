// What the popup and the command palette ask the service worker, and its answers; what pages
// tell and ask it; and what it tells pages.
import type { SubmittedLogin } from "./capture";
import type { EntrySummary, PageEntries } from "./companion";
import type { FailureReason, FoundEntry, Status } from "./keefast";

/** What the popup asks. */
export type Request =
  | { type: "status" }
  | { type: "pair" }
  | { type: "entries"; tab: number }
  | { type: "fill"; tab: number; uuid: string }
  | { type: "generate"; tab: number }
  | { type: "fill-generated"; tab: number }
  /** Shows keefast in the sign-in fields of the tab's site again, after the user hid it. */
  | { type: "show-on-site"; tab: number };

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
  "show-on-site": null;
}

/** What the command palette is open for. */
export type PaletteMode = "entries" | "generate";

/** What the command palette asks, for the tab it was opened in. */
export type PaletteRequest =
  | { type: "open" }
  | { type: "search"; query: string }
  /** Fills an Entry listed for the site, or with `found`, one the search found. */
  | { type: "fill"; uuid: string; found: boolean }
  | { type: "generate" }
  | { type: "fill-generated" }
  /** Stops showing keefast in the site's sign-in fields, and closes the palette. */
  | { type: "hide-site" }
  | { type: "close" };

/** What the palette shows first. */
export interface PaletteStart {
  mode: PaletteMode;
  /** The host of the site the palette fills. */
  site: string;
  /** The site's Entries, when open for them. */
  entries: EntrySummary[];
}

export interface PaletteAnswers {
  open: PaletteStart;
  search: FoundEntry[];
  fill: boolean;
  generate: string;
  "fill-generated": boolean;
  "hide-site": null;
  close: null;
}

/** A palette's request, with the token that proves keefast opened that palette. */
export interface PaletteMessage {
  type: "palette";
  token: string;
  request: PaletteRequest;
}

/** The kinds of field keefast shows its icon in. */
export type FieldKind = "login" | "totp" | "new-password";

/** What clicking keefast's icon in a field did. */
export type FieldClickAnswer =
  /** Filled the site's only Entry. */
  | { kind: "filled" }
  /** Opened the command palette. */
  | { kind: "palette" }
  /** Did nothing: keefast can't fill the page, or isn't ready, as `status` says. */
  | { kind: "unavailable"; status: Exclude<Status, "paired"> | "failed" };

/** What the scripts in a page tell and ask the service worker. */
export type PageMessage =
  /** A login the user submitted in a page the user filled. */
  | { type: "submitted"; login: SubmittedLogin; submitUrl?: string }
  /** Whether to show keefast in the page's sign-in fields: the user may have hidden it. */
  | { type: "fields-start" }
  /** The user clicked keefast's icon in a field. */
  | { type: "field-click"; field: FieldKind }
  /** The user asked, from a field's icon, to connect the browser to keefast. */
  | { type: "connect" };

export interface PageAnswers {
  submitted: null;
  "fields-start": boolean;
  "field-click": FieldClickAnswer;
  connect: Answer<null>;
}

/** What the service worker tells a page's top frame. */
export type TabMessage =
  /** Frames the command palette at `url` over the page. */
  | { type: "open-palette"; url: string }
  | { type: "close-palette" }
  /** Removes keefast's icons from the page's fields: the user hid keefast on the site. */
  | { type: "hide-fields" };
