import { describe, expect, it } from "vitest";

import { monthYearState } from "./fields";

describe("monthYearState", () => {
  const now = new Date(Date.UTC(2026, 9, 10)); // 10 October 2026

  it("is good through the month's last day", () => {
    expect(monthYearState("10/26", now)).toBe("soon");
    expect(monthYearState("09/26", now)).toBe("expired");
    expect(monthYearState("10/26", new Date(Date.UTC(2026, 10, 1)))).toBe("expired");
  });

  it("is soon within 60 days, and fine after", () => {
    expect(monthYearState("11/26", now)).toBe("soon");
    expect(monthYearState("12/26", now)).toBe("ok");
    expect(monthYearState("08/31", now)).toBe("ok");
  });

  it("says when it isn't MM/YY", () => {
    expect(monthYearState("", now)).toBe("empty");
    expect(monthYearState("13/26", now)).toBe("invalid");
    expect(monthYearState("8/31", now)).toBe("invalid");
    expect(monthYearState("2031-08", now)).toBe("invalid");
  });
});
