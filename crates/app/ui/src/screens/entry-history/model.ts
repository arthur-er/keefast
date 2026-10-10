// Entry history B's model: the Entry's versions newest first (the current one, then its history),
// what changed between two of them, and how the screen words it. Pure, so it is tested alone.
//
// Versions carry no secrets. Whether a password or protected custom field differs between two
// versions comes from the backend (`passwordIsCurrent`, `passwordIsPrevious`,
// `protectedChangedFrom…`), so comparing never reveals anything; the values are revealed only
// when the user asks to see them.

import type { Color, CustomFieldView, EntryDetail, EntryVersion, TotpState } from "@/bindings";

/** A version: `"current"` for the Entry as it is now, else its index in the history. */
export type VersionKey = "current" | number;

export interface Version {
  key: VersionKey;
  modified: number | null;
  title: string;
  username: string;
  url: string;
  notes: string;
  hasPassword: boolean;
  totp: TotpState;
  tags: string[];
  color: Color | null;
  customFields: CustomFieldView[];
  attachments: string[];
  /** The backend's comparisons of its secrets, for a version in the history. */
  saved: EntryVersion | null;
}

export function currentVersion(entry: EntryDetail): Version {
  return {
    key: "current",
    modified: entry.modified,
    title: entry.title,
    username: entry.username,
    url: entry.url,
    notes: entry.notes,
    hasPassword: entry.hasPassword,
    totp: entry.totp === null ? "none" : entry.totp.status === "ready" ? "set" : "unreadable",
    tags: entry.tags,
    color: entry.color,
    customFields: entry.customFields,
    attachments: entry.attachments.map((attachment) => attachment.name),
    saved: null,
  };
}

export function savedVersion(version: EntryVersion): Version {
  return { ...version, key: version.index, saved: version };
}

/** Every version, newest first: the current one, then the history from its newest. */
export function allVersions(entry: EntryDetail, history: readonly EntryVersion[]): Version[] {
  return [currentVersion(entry), ...[...history].sort((a, b) => b.index - a.index).map(savedVersion)];
}

/** Where a secret value is, to reveal it. `field` is a custom field's index in that version. */
export interface SecretRef {
  version: VersionKey;
  field: "password" | number;
}

export type Value =
  | { kind: "empty" }
  | { kind: "text"; text: string; mono?: boolean }
  | { kind: "secret"; ref: SecretRef };

export type FieldKey = "title" | "username" | "password" | "url" | "notes" | "totp" | "tags" | "color" | "attachments" | `field:${string}`;

export interface Row {
  key: FieldKey;
  label: string;
  /** The value in the version shown. */
  value: Value;
  /** When it differs from the version compared with: its value there, then this one's. */
  change: { from: Value; to: Value } | null;
}

const TOTP_TEXT: Record<TotpState, string> = { none: "Not set up", set: "Set up", unreadable: "Set up, but can't be read" };

const text = (value: string, mono = false): Value => (value === "" ? { kind: "empty" } : { kind: "text", text: value, mono });
const list = (items: readonly string[]): Value => (items.length === 0 ? { kind: "empty" } : { kind: "text", text: items.join(", ") });

function fieldValue(version: Version, index: number): Value {
  const field = version.customFields[index];
  if (!field) return { kind: "empty" };
  if (field.protected) return field.isEmpty ? { kind: "empty" } : { kind: "secret", ref: { version: version.key, field: index } };
  return text(field.value ?? "");
}

/** Whether two versions' passwords differ. `a` and `b` are the current version and another,
 * or two versions next to each other. */
export function passwordsDiffer(a: Version, b: Version): boolean {
  if (a.hasPassword !== b.hasPassword) return true;
  if (!a.hasPassword) return false;
  const [newer, older] = order(a, b);
  if (newer.key === "current") return !older.saved!.passwordIsCurrent;
  if (older.key === "current") return !newer.saved!.passwordIsCurrent;
  // Next to each other: the newer one knows about the one before it.
  if (newer.saved && older.saved && newer.saved.index === older.saved.index + 1) return !newer.saved.passwordIsPrevious;
  // Two versions apart: what each says against the current one is all there is to go on.
  return newer.saved!.passwordIsCurrent !== older.saved!.passwordIsCurrent;
}

/** The protected custom fields both versions have whose values differ. */
function protectedChanges(a: Version, b: Version): Set<string> {
  const [newer, older] = order(a, b);
  if (newer.key === "current") return new Set(older.saved!.protectedChangedFromCurrent);
  if (older.key === "current") return new Set(newer.saved!.protectedChangedFromCurrent);
  if (newer.saved!.index === older.saved!.index + 1) return new Set(newer.saved!.protectedChangedFromPrevious);
  // Two versions apart: a field one changed against the current one, the other didn't.
  const one = new Set(newer.saved!.protectedChangedFromCurrent);
  const other = new Set(older.saved!.protectedChangedFromCurrent);
  return new Set([...one, ...other].filter((name) => one.has(name) !== other.has(name)));
}

/** Newer first. */
function order(a: Version, b: Version): [Version, Version] {
  if (a.key === "current") return [a, b];
  if (b.key === "current") return [b, a];
  return a.key > b.key ? [a, b] : [b, a];
}

/**
 * The rows comparing `version` with `base`: every field `version` has (and custom fields only
 * `base` has), each with its change from `base` to `version` when it has one. With no base (the
 * oldest version against the one before it), nothing changed.
 */
export function compare(version: Version, base: Version | null): Row[] {
  const rows: Row[] = [];
  const add = (key: FieldKey, label: string, value: Value, from: Value | null, changed: boolean) =>
    rows.push({ key, label, value, change: base && changed && from ? { from, to: value } : null });

  add("title", "Title", text(version.title), base && text(base.title), !!base && base.title !== version.title);
  add("username", "Username", text(version.username), base && text(base.username), !!base && base.username !== version.username);
  const password = (v: Version): Value => (v.hasPassword ? { kind: "secret", ref: { version: v.key, field: "password" } } : { kind: "empty" });
  add("password", "Password", password(version), base && password(base), !!base && passwordsDiffer(base, version));
  add("url", "URL", text(version.url), base && text(base.url), !!base && base.url !== version.url);
  add("notes", "Notes", text(version.notes), base && text(base.notes), !!base && base.notes !== version.notes);
  const totp = (v: Version): Value => ({ kind: "text", text: TOTP_TEXT[v.totp] });
  add("totp", "TOTP", totp(version), base && totp(base), !!base && base.totp !== version.totp);
  add("tags", "Tags", list(version.tags), base && list(base.tags), !!base && base.tags.join("\n") !== version.tags.join("\n"));
  if (version.color !== null || base?.color) {
    const color = (v: Version): Value => (v.color === null ? { kind: "empty" } : { kind: "text", text: v.color[0]!.toUpperCase() + v.color.slice(1) });
    add("color", "Colour", color(version), base && color(base), !!base && base.color !== version.color);
  }

  const secrets = base ? protectedChanges(base, version) : new Set<string>();
  const names = [...version.customFields.map((field) => field.name)];
  for (const field of base?.customFields ?? []) if (!names.includes(field.name)) names.push(field.name);
  for (const name of names) {
    const here = version.customFields.findIndex((field) => field.name === name);
    const there = base ? base.customFields.findIndex((field) => field.name === name) : -1;
    const value: Value = here < 0 ? { kind: "empty" } : fieldValue(version, here);
    const from: Value = !base || there < 0 ? { kind: "empty" } : fieldValue(base, there);
    let changed = false;
    if (base) {
      const [a, b] = [version.customFields[here], base.customFields[there]];
      if (!a || !b) changed = true;
      else {
        changed = a.protected || b.protected ? secrets.has(name) || a.isEmpty !== b.isEmpty : a.value !== b.value;
      }
    }
    add(`field:${name}`, name, value, from, changed);
  }

  add("attachments", "Attachments", list(version.attachments), base && list(base.attachments), !!base && base.attachments.join("\n") !== version.attachments.join("\n"));
  return rows;
}

/** What changed from `older` to `newer`, in a few words each: "Password changed", "Tagged dev".
 * With no older version, the Entry was created. */
export function summarize(newer: Version, older: Version | null): string[] {
  if (!older) return ["Entry created"];
  const words: string[] = [];
  for (const row of compare(newer, older)) {
    if (!row.change) continue;
    const isEmpty = (value: Value) => value.kind === "empty";
    switch (row.key) {
      case "totp":
        words.push(newer.totp === "none" ? "TOTP removed" : older.totp === "none" ? "TOTP set up" : "TOTP changed");
        break;
      case "tags": {
        const added = newer.tags.filter((tag) => !older.tags.includes(tag));
        words.push(added.length > 0 ? `Tagged ${added.join(", ")}` : newer.tags.length < older.tags.length ? "Tags removed" : "Tags changed");
        break;
      }
      case "attachments":
        words.push(newer.attachments.length > older.attachments.length ? "Attachment added" : newer.attachments.length < older.attachments.length ? "Attachment removed" : "Attachments changed");
        break;
      default:
        if (row.key.startsWith("field:") || row.key === "notes") {
          const label = row.key === "notes" ? "Notes" : row.label;
          words.push(isEmpty(row.change.from) ? `${label} added` : isEmpty(row.change.to) ? `${label} removed` : `${label} changed`);
        } else {
          words.push(`${row.label} changed`);
        }
    }
  }
  return words.length > 0 ? words : ["No change to its fields"];
}

/** The rows that changed. */
export const changedCount = (rows: readonly Row[]) => rows.filter((row) => row.change).length;

const DAY = 86400;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const time = (date: Date) => `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;

/** "Jun 3, 2026, 21:02". */
export function longTime(seconds: number | null): string {
  if (seconds === null) return "an unknown time";
  const date = new Date(seconds * 1000);
  return `${MONTHS[date.getMonth()]} ${date.getDate()}, ${date.getFullYear()}, ${time(date)}`;
}

/** "Oct 7, 14:30": the list's headings say the year. */
export function shortTime(seconds: number | null): string {
  if (seconds === null) return "an unknown time";
  const date = new Date(seconds * 1000);
  return `${MONTHS[date.getMonth()]} ${date.getDate()}, ${time(date)}`;
}

/** "Today", "3 days ago", "4 months ago", "2 years ago". */
export function relativeTime(seconds: number | null, now = Date.now() / 1000): string {
  if (seconds === null) return "Unknown time";
  const days = Math.floor((now - seconds) / DAY);
  if (days < 1) return "Today";
  if (days < 2) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  if (days < 60) return `${Math.round(days / 7)} weeks ago`;
  if (days < 365) return `${Math.round(days / 30)} months ago`;
  const years = Math.round(days / 365);
  return `${years} year${years === 1 ? "" : "s"} ago`;
}

/** The heading a version is listed under: "Now", "This week", "Earlier this year", "2025". */
export function bucket(version: Version, now = Date.now() / 1000): string {
  if (version.key === "current") return "Now";
  if (version.modified === null) return "Unknown date";
  if (now - version.modified < 7 * DAY) return "This week";
  const year = new Date(version.modified * 1000).getFullYear();
  return year === new Date(now * 1000).getFullYear() ? "Earlier this year" : String(year);
}

/** The version to compare with: the one before it, or the current one. Null for the oldest
 * against the one before it (there is none), and the current one against itself. */
export function baseOf(versions: readonly Version[], selected: number, against: "previous" | "current"): Version | null {
  if (against === "current") return selected === 0 ? null : (versions[0] ?? null);
  return versions[selected + 1] ?? null;
}
