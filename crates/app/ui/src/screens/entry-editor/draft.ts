// Entry editor A's form, as plain data: the draft made from an Entry (or a new one), whether it
// changed, what is wrong with it, and the `EntryInput` it saves as.
//
// Secrets stay in the backend until the user wants them: the password and protected custom
// fields start as `null` ("kept as they are") and are only revealed when the user shows or edits
// them. A revealed value the user didn't change still saves as kept.

import type { Color, CustomFieldInput, EntryDetail, EntryInput, IconChoice } from "@/bindings";

/** The fields TOTP settings are kept in (`TOTP_FIELDS` in crates/core/src/totp.rs). The editor
 * shows them as the TOTP row rather than as custom fields, and saves them as they are. */
export const TOTP_FIELDS: readonly string[] = [
  "otp",
  "TOTP Seed",
  "TOTP Settings",
  "TimeOtp-Secret-Base32",
  "TimeOtp-Secret",
  "TimeOtp-Secret-Hex",
  "TimeOtp-Secret-Base64",
  "TimeOtp-Length",
  "TimeOtp-Period",
  "TimeOtp-Algorithm",
];

/** The standard fields every Entry has, which no custom field may be named. */
export const STANDARD_FIELDS: readonly string[] = ["Title", "UserName", "Password", "URL", "Notes"];

export interface FieldRow {
  /** Stable while editing, for React. */
  key: number;
  name: string;
  /** `null`: the saved value, not revealed. */
  value: string | null;
  protected: boolean;
  /** The field's name in the saved Entry, for a field it had; `null` for a new one. */
  original: string | null;
  /** The saved value, once revealed: a value equal to it saves as kept. */
  savedValue: string | null;
}

export interface Draft {
  title: string;
  username: string;
  /** `null`: the saved password, not revealed. */
  password: string | null;
  /** The saved password, once revealed. */
  savedPassword: string | null;
  url: string;
  notes: string;
  fields: FieldRow[];
  tags: string[];
  color: Color | null;
  icon: IconChoice;
  expires: boolean;
  /** `YYYY-MM-DD`, also kept while expiry is off, to turn it back on. */
  expiry: string;
}

let nextKey = 1;
export const rowKey = () => nextKey++;

/** The draft of an Entry as it is saved. TOTP's own fields are left out while it has TOTP. */
export function draftOf(entry: EntryDetail): Draft {
  const hideTotp = entry.totp !== null;
  return {
    title: entry.rawTitle,
    username: entry.username,
    password: entry.hasPassword ? null : "",
    savedPassword: entry.hasPassword ? null : "",
    url: entry.url,
    notes: entry.notes,
    fields: entry.customFields
      .filter((field) => !(hideTotp && TOTP_FIELDS.includes(field.name)))
      .map((field) => ({
        key: rowKey(),
        name: field.name,
        value: field.value ?? (field.isEmpty ? "" : null),
        protected: field.protected,
        original: field.name,
        savedValue: field.value ?? (field.isEmpty ? "" : null),
      })),
    tags: [...entry.tags],
    color: entry.color,
    icon: entry.icon.kind === "custom" ? { kind: "custom", id: entry.icon.id } : { kind: "standard", number: entry.icon.number },
    expires: entry.expires !== null,
    expiry: entry.expires !== null ? isoDate(entry.expires) : "",
  };
}

/** A new Entry's draft. Made while a tag or a colour is shown, it has it, so it shows there. */
export function newDraft(defaults: { tags?: string[]; color?: Color | null } = {}): Draft {
  return {
    title: "",
    username: "",
    password: "",
    savedPassword: "",
    url: "",
    notes: "",
    fields: [],
    tags: defaults.tags ?? [],
    color: defaults.color ?? null,
    icon: { kind: "standard", number: 0 },
    expires: false,
    expiry: "",
  };
}

/** A date as `YYYY-MM-DD`, in UTC as the backend reads it. Times are seconds since 1970. */
export function isoDate(seconds: number): string {
  return new Date(seconds * 1000).toISOString().slice(0, 10);
}

/** `YYYY-MM-DD` `days` after `from` (seconds since 1970). */
export function dateIn(days: number, from = Date.now() / 1000): string {
  return isoDate(from + days * 86400);
}

export function validDate(text: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(text)) return false;
  const date = new Date(`${text}T00:00:00Z`);
  return !Number.isNaN(date.getTime()) && date.toISOString().slice(0, 10) === text;
}

/** Whether the draft's expiry has passed by `now` (seconds). */
export function draftExpired(draft: Draft, now = Date.now() / 1000): boolean {
  return draft.expires && validDate(draft.expiry) && Date.parse(`${draft.expiry}T00:00:00Z`) / 1000 <= now;
}

/** Tags typed into the tag field: split on commas and semicolons, as KeePass does. */
export function splitTags(text: string): string[] {
  return text
    .split(/[,;]/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

/** `tags` with the ones in `text` added, each once. */
export function addTags(tags: readonly string[], text: string): string[] {
  const next = [...tags];
  for (const tag of splitTags(text)) if (!next.includes(tag)) next.push(tag);
  return next;
}

/** What the draft would change, comparable with `===` after `JSON.stringify`. */
function comparable(draft: Draft) {
  return {
    title: draft.title,
    username: draft.username,
    password: draft.password === null || draft.password === draft.savedPassword ? null : draft.password,
    url: draft.url,
    notes: draft.notes,
    fields: draft.fields.map((row) => ({
      name: row.name,
      value: row.value === null || row.value === row.savedValue ? null : row.value,
      protected: row.protected,
      original: row.original,
    })),
    tags: draft.tags,
    color: draft.color,
    icon: draft.icon,
    expiry: draft.expires ? draft.expiry : null,
  };
}

/** Whether `draft` differs from `saved`, the draft it started as. */
export function isDirty(saved: Draft, draft: Draft): boolean {
  return JSON.stringify(comparable(saved)) !== JSON.stringify(comparable(draft));
}

export type DraftError = "fieldNameEmpty" | "fieldNameTaken" | "expiry";

export const ERROR_TEXT: Record<DraftError, string> = {
  fieldNameEmpty: "Give every custom field a name.",
  fieldNameTaken: "Each field needs a name of its own, other than Title, UserName, Password, URL or Notes.",
  expiry: "Type the expiry date as YYYY-MM-DD, such as 2027-01-31.",
};

export interface Problems {
  errors: DraftError[];
  /** Keys of the custom field rows whose names are wrong. */
  fields: Set<number>;
  expiry: boolean;
}

/** What keeps the draft from saving, as the backend would refuse it; `null` when nothing.
 * `hidden` are the names of fields saved as they are (TOTP's). */
export function problems(draft: Draft, hidden: readonly string[] = []): Problems | null {
  const found: Problems = { errors: [], fields: new Set(), expiry: false };
  const add = (error: DraftError) => found.errors.includes(error) || found.errors.push(error);
  const seen = new Set(hidden);
  for (const row of draft.fields) {
    const name = row.name.trim();
    if (!name) {
      found.fields.add(row.key);
      add("fieldNameEmpty");
      continue;
    }
    if (STANDARD_FIELDS.includes(name) || seen.has(name)) {
      found.fields.add(row.key);
      add("fieldNameTaken");
    }
    seen.add(name);
  }
  if (draft.expires && !validDate(draft.expiry)) {
    found.expiry = true;
    add("expiry");
  }
  return found.errors.length ? found : null;
}

/** The fields of `entry` the editor doesn't show (TOTP's), to save as they are. */
export function hiddenFields(entry: EntryDetail | null, draft: Draft): { name: string; index: number; protected: boolean }[] {
  if (!entry) return [];
  const shown = new Set(draft.fields.flatMap((row) => (row.original === null ? [] : [row.original])));
  return entry.customFields.flatMap((field, index) =>
    TOTP_FIELDS.includes(field.name) && !shown.has(field.name) ? [{ name: field.name, index, protected: field.protected }] : [],
  );
}

/** The index of the saved field named `name` in `entry`, as it is now. */
export function savedIndex(entry: EntryDetail | null, name: string | null): number | null {
  if (!entry || name === null) return null;
  const index = entry.customFields.findIndex((field) => field.name === name);
  return index < 0 ? null : index;
}

/** The `EntryInput` the draft saves as. `entry` is the Entry as it is now (`null` for a new
 * one): kept values are found in it by name, so its fields may have moved since editing began. */
export function toInput(draft: Draft, entry: EntryDetail | null): EntryInput {
  const kept = (value: string | null, saved: string | null) => value === null || (saved !== null && value === saved);
  const fields: CustomFieldInput[] = draft.fields.map((row) => {
    const index = savedIndex(entry, row.original);
    const keep = index !== null && kept(row.value, row.savedValue);
    return { name: row.name.trim(), value: keep ? null : (row.value ?? ""), protected: row.protected, original: keep ? index : null };
  });
  for (const hidden of hiddenFields(entry, draft)) {
    fields.push({ name: hidden.name, value: null, protected: hidden.protected, original: hidden.index });
  }
  return {
    title: draft.title,
    username: draft.username,
    password: kept(draft.password, draft.savedPassword) ? null : draft.password,
    url: draft.url,
    notes: draft.notes,
    customFields: fields,
    tags: draft.tags,
    color: draft.color,
    icon: draft.icon,
    expires: draft.expires ? draft.expiry : null,
  };
}
