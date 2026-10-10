import { describe, expect, it } from "vitest";

import type { CustomFieldView, EntryDetail, EntryVersion } from "@/bindings";

import { allVersions, baseOf, bucket, compare, relativeTime, summarize } from "./model";

const pat = (value: string | null): CustomFieldView => ({ name: "PAT", protected: value === null, value, isEmpty: false, totpStorage: false });

function saved(index: number, changes: Partial<EntryVersion> = {}): EntryVersion {
  return {
    index,
    modified: 1_000_000 + index * 1000,
    title: "GitHub",
    username: "arthur",
    url: "https://github.com",
    notes: "",
    hasPassword: true,
    passwordIsCurrent: true,
    passwordIsPrevious: true,
    customFields: [],
    tags: [],
    color: null,
    totp: "none",
    attachments: [],
    protectedChangedFromPrevious: [],
    protectedChangedFromCurrent: [],
    ...changes,
  };
}

const current = {
  vault: 1,
  id: "e1",
  title: "GitHub",
  username: "arthur",
  url: "https://github.com",
  notes: "",
  hasPassword: true,
  totp: { status: "ready", digits: 6, period: 30, algorithm: "sha1", steam: false },
  tags: ["dev", "2fa"],
  color: null,
  customFields: [pat(null)],
  attachments: [],
  modified: 1_010_000,
  created: 1_000_000,
} as unknown as EntryDetail;

// Oldest first, as the backend sends them: created; password changed; PAT added; tagged dev.
const history = [
  saved(0, { passwordIsCurrent: false, passwordIsPrevious: false }),
  saved(1, { passwordIsPrevious: false }),
  saved(2, { customFields: [pat(null)] }),
  saved(3, { customFields: [pat(null)], tags: ["dev"], protectedChangedFromCurrent: [] }),
];

describe("the versions", () => {
  const versions = allVersions(current, history);

  it("are listed newest first, the current one on top", () => {
    expect(versions.map((version) => version.key)).toEqual(["current", 3, 2, 1, 0]);
  });

  it("say what each changed from the one before", () => {
    expect(summarize(versions[0]!, versions[1]!)).toEqual(["TOTP set up", "Tagged 2fa"]);
    expect(summarize(versions[1]!, versions[2]!)).toEqual(["Tagged dev"]);
    expect(summarize(versions[2]!, versions[3]!)).toEqual(["PAT added"]);
    expect(summarize(versions[3]!, versions[4]!)).toEqual(["Password changed"]);
    expect(summarize(versions[4]!, null)).toEqual(["Entry created"]);
  });

  it("tell a changed password apart without its value, against the current one too", () => {
    const password = (rows: ReturnType<typeof compare>) => rows.find((row) => row.key === "password")!;
    expect(password(compare(versions[3]!, versions[4]!)).change).not.toBeNull();
    expect(password(compare(versions[2]!, versions[3]!)).change).toBeNull();
    // Against the current one: only the oldest has another password.
    expect(password(compare(versions[4]!, versions[0]!)).change).not.toBeNull();
    expect(password(compare(versions[3]!, versions[0]!)).change).toBeNull();
  });

  it("show a protected field's change as secrets to reveal", () => {
    const changedPat = allVersions(current, [saved(0, { customFields: [pat(null)], protectedChangedFromCurrent: ["PAT"] })]);
    const row = compare(changedPat[1]!, changedPat[0]!).find((r) => r.key === "field:PAT")!;
    expect(row.change).toEqual({
      from: { kind: "secret", ref: { version: "current", field: 0 } },
      to: { kind: "secret", ref: { version: 0, field: 0 } },
    });
    const samePat = allVersions(current, [saved(0, { customFields: [pat(null)] })]);
    expect(compare(samePat[1]!, samePat[0]!).find((r) => r.key === "field:PAT")!.change).toBeNull();
  });

  it("list a field only the base has, as removed", () => {
    const row = compare(versions[3]!, versions[0]!).find((r) => r.key === "field:PAT")!;
    expect(row.value).toEqual({ kind: "empty" });
    expect(row.change?.to).toEqual({ kind: "empty" });
  });

  it("compare with the previous version or the current one", () => {
    expect(baseOf(versions, 1, "previous")?.key).toBe(2);
    expect(baseOf(versions, 4, "previous")).toBeNull();
    expect(baseOf(versions, 2, "current")?.key).toBe("current");
    expect(baseOf(versions, 0, "current")).toBeNull();
  });
});

describe("dates", () => {
  const now = Date.parse("2026-10-09T10:00:00") / 1000;
  const at = (date: string) => Date.parse(date) / 1000;

  it("group versions as the prototype does", () => {
    const version = (modified: number) => ({ ...allVersions(current, [saved(0, { modified })])[1]! });
    expect(bucket(allVersions(current, [])[0]!, now)).toBe("Now");
    expect(bucket(version(at("2026-10-07T14:30:00")), now)).toBe("This week");
    expect(bucket(version(at("2026-03-14T08:15:00")), now)).toBe("Earlier this year");
    expect(bucket(version(at("2025-11-02T18:40:00")), now)).toBe("2025");
  });

  it("say how long ago", () => {
    expect(relativeTime(at("2026-06-03T21:02:00"), now)).toBe("4 months ago");
    expect(relativeTime(at("2026-10-08T21:02:00"), now)).toBe("Today");
    expect(relativeTime(at("2024-03-02T10:04:00"), now)).toBe("3 years ago");
  });
});
