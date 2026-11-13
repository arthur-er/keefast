import { describe, expect, it } from "vitest";

import { describeExpiry, formatDate, formatSize, highlight, hostOf, timeAgo } from "./format";

const at = (iso: string) => Date.parse(iso) / 1000;
const NOW = at("2026-10-09T12:00:00");

describe("formatDate", () => {
  it("leaves the year out this year", () => {
    expect(formatDate(at("2026-10-07T12:00:00"), NOW)).toBe("Oct 7");
    expect(formatDate(at("2021-04-18T12:00:00"), NOW)).toBe("Apr 18, 2021");
  });
});

describe("describeExpiry", () => {
  it("tells never, expired, soon and later apart", () => {
    expect(describeExpiry(null, false, NOW)).toEqual({ kind: "never" });
    expect(describeExpiry(at("2026-09-01T12:00:00"), true, NOW).kind).toBe("expired");
    // Expired by now, though the list said otherwise when it was fetched.
    expect(describeExpiry(at("2026-10-09T11:00:00"), false, NOW).kind).toBe("expired");
    expect(describeExpiry(at("2026-10-20T12:00:00"), false, NOW)).toEqual({ kind: "soon", date: "Oct 20", days: 11 });
    expect(describeExpiry(at("2027-01-31T12:00:00"), false, NOW)).toEqual({ kind: "later", date: "Jan 31, 2027" });
  });
});

describe("timeAgo", () => {
  it("counts minutes, then hours, then days", () => {
    const now = NOW;
    expect(timeAgo(now - 20, now)).toBe("just now");
    expect(timeAgo(now + 5, now)).toBe("just now");
    expect(timeAgo(now - 60, now)).toBe("1 min ago");
    expect(timeAgo(now - 59 * 60, now)).toBe("59 min ago");
    expect(timeAgo(now - 60 * 60, now)).toBe("1 hour ago");
    expect(timeAgo(now - 5 * 3600, now)).toBe("5 hours ago");
    expect(timeAgo(now - 86400, now)).toBe("1 day ago");
    expect(timeAgo(now - 3 * 86400, now)).toBe("3 days ago");
  });
});

describe("hostOf", () => {
  it("reads hosts out of URLs and bare addresses", () => {
    expect(hostOf("https://github.com/login")).toBe("github.com");
    expect(hostOf("postgres://10.0.4.12:5432")).toBe("10.0.4.12:5432");
    expect(hostOf("github.com/login")).toBe("github.com");
    expect(hostOf("")).toBe("");
  });
});

describe("formatSize", () => {
  it("rounds to one decimal above a kilobyte", () => {
    expect(formatSize(411)).toBe("411 B");
    expect(formatSize(1229)).toBe("1.2 KB");
  });
});

describe("highlight", () => {
  it("finds the query in any case", () => {
    expect(highlight("GitHub", "hub")).toEqual({ before: "Git", match: "Hub", after: "" });
    expect(highlight("GitHub", "  ")).toBeNull();
    expect(highlight("GitHub", "lab")).toBeNull();
  });
});
