import { describe, expect, it } from "vitest";

import type { EntryDetail } from "@/bindings";

import { addTags, draftExpired, draftOf, isDirty, newDraft, problems, splitTags, toInput, validDate } from "./draft";
import { filterStandardIcons, STANDARD_ICON_NAMES } from "./icons";

const entry = (overrides: Partial<EntryDetail> = {}): EntryDetail => ({
  vault: 1,
  id: "e1",
  title: "GitHub",
  rawTitle: "GitHub",
  username: "arthur-er",
  url: "https://github.com",
  tags: ["dev"],
  color: "violet",
  icon: { kind: "standard", number: 1 },
  faviconUrl: null,
  expires: Date.UTC(2027, 0, 31) / 1000,
  expired: false,
  created: null,
  modified: null,
  hasPassword: true,
  hasTotp: true,
  attachmentCount: 0,
  group: "g",
  groupName: "Personal",
  vaultName: "Personal",
  inRecycleBin: false,
  notes: "",
  additionalUrls: [],
  customFields: [
    { name: "PAT", protected: true, value: null, isEmpty: false },
    { name: "otp", protected: true, value: null, isEmpty: false },
    { name: "Account", protected: false, value: "42", isEmpty: false },
  ],
  attachments: [],
  totp: { status: "ready", digits: 6, period: 30, algorithm: "sha1", steam: false },
  historyCount: 0,
  rememberedAccess: [],
  groupPath: ["Personal"],
  deletesForGood: false,
  ...overrides,
});

describe("draftOf", () => {
  it("keeps secrets in the backend and leaves TOTP's fields to the TOTP row", () => {
    const draft = draftOf(entry());
    expect(draft.password).toBeNull();
    expect(draft.fields.map((row) => [row.name, row.value])).toEqual([
      ["PAT", null],
      ["Account", "42"],
    ]);
    expect(draft.expiry).toBe("2027-01-31");
  });

  it("shows TOTP's fields as custom fields when the Entry has no TOTP", () => {
    expect(draftOf(entry({ totp: null })).fields.map((row) => row.name)).toEqual(["PAT", "otp", "Account"]);
  });
});

describe("isDirty", () => {
  it("doesn't count a revealed secret the user left alone", () => {
    const saved = draftOf(entry());
    const revealed = { ...saved, password: "hunter2", savedPassword: "hunter2" };
    expect(isDirty(saved, revealed)).toBe(false);
    expect(isDirty(saved, { ...revealed, password: "hunter3" })).toBe(true);
  });

  it("counts tags, colour and expiry", () => {
    const saved = draftOf(entry());
    expect(isDirty(saved, { ...saved, tags: [...saved.tags, "2fa"] })).toBe(true);
    expect(isDirty(saved, { ...saved, color: null })).toBe(true);
    expect(isDirty(saved, { ...saved, expires: false })).toBe(true);
    // A date typed while expiry is off changes nothing.
    const off = { ...saved, expires: false };
    expect(isDirty(off, { ...off, expiry: "2030-01-01" })).toBe(false);
  });
});

describe("problems", () => {
  it("finds empty, standard and repeated field names, and a bad date", () => {
    const draft = newDraft();
    const rows = ["", "Password", "Recovery", "Recovery"].map((name, key) => ({
      key,
      name,
      value: "",
      protected: false,
      original: null,
      savedValue: null,
    }));
    const found = problems({ ...draft, fields: rows, expires: true, expiry: "2026-13-01" });
    expect(found?.errors).toEqual(["fieldNameEmpty", "fieldNameTaken", "expiry"]);
    expect([...found!.fields]).toEqual([0, 1, 3]);
  });

  it("keeps names TOTP's fields use for them", () => {
    const draft = newDraft();
    const row = { key: 1, name: "otp", value: "", protected: false, original: null, savedValue: null };
    expect(problems({ ...draft, fields: [row] }, ["otp"])?.errors).toEqual(["fieldNameTaken"]);
    expect(problems({ ...draft, fields: [row] })).toBeNull();
  });
});

describe("toInput", () => {
  it("keeps unrevealed values by their name in the Entry as it is now", () => {
    const before = entry();
    const draft = draftOf(before);
    // TOTP was set up again meanwhile: its field moved to the end.
    const now = entry({
      customFields: [before.customFields[0]!, before.customFields[2]!, { name: "otp", protected: true, value: null, isEmpty: false }],
    });
    const input = toInput({ ...draft, title: "GitHub (work)" }, now);
    expect(input.password).toBeNull();
    expect(input.customFields).toEqual([
      { name: "PAT", value: null, protected: true, original: 0 },
      { name: "Account", value: null, protected: false, original: 1 },
      { name: "otp", value: null, protected: true, original: 2 },
    ]);
    expect(input.expires).toBe("2027-01-31");
  });

  it("sends what the user changed", () => {
    const draft = draftOf(entry());
    const [pat, account] = draft.fields;
    const input = toInput(
      {
        ...draft,
        password: "new",
        fields: [{ ...pat!, value: "ghp_new", savedValue: "ghp_old" }, { ...account!, name: " Account no. " }],
        expires: false,
      },
      entry(),
    );
    expect(input.password).toBe("new");
    expect(input.customFields.slice(0, 2)).toEqual([
      { name: "PAT", value: "ghp_new", protected: true, original: null },
      { name: "Account no.", value: null, protected: false, original: 2 },
    ]);
    expect(input.expires).toBeNull();
  });

  it("makes a new Entry with no password unless one is typed", () => {
    expect(toInput(newDraft({ tags: ["work"], color: "red" }), null)).toMatchObject({ password: null, tags: ["work"], color: "red", customFields: [] });
    expect(toInput({ ...newDraft(), password: "pw" }, null).password).toBe("pw");
  });
});

describe("dates and tags", () => {
  it("reads only real YYYY-MM-DD dates", () => {
    expect(validDate("2027-01-31")).toBe(true);
    expect(validDate("2026-13-01")).toBe(false);
    expect(validDate("2027-02-30")).toBe(false);
    expect(validDate("31/01/2027")).toBe(false);
  });

  it("says when the expiry has passed", () => {
    const draft = { ...newDraft(), expires: true, expiry: "2026-10-01" };
    expect(draftExpired(draft, Date.UTC(2026, 9, 10) / 1000)).toBe(true);
    expect(draftExpired(draft, Date.UTC(2026, 8, 30) / 1000)).toBe(false);
    expect(draftExpired({ ...draft, expires: false }, Date.UTC(2026, 9, 10) / 1000)).toBe(false);
  });

  it("splits tags on commas and semicolons, each once", () => {
    expect(splitTags(" a, b;c ,, ")).toEqual(["a", "b", "c"]);
    expect(addTags(["a"], "a, b")).toEqual(["a", "b"]);
  });
});

describe("standard icons", () => {
  it("names all 69 and filters on the names", () => {
    expect(STANDARD_ICON_NAMES).toHaveLength(69);
    expect(filterStandardIcons("")).toHaveLength(69);
    expect(filterStandardIcons("folder")).toEqual([48, 49, 50]);
  });
});
