import { describe, expect, it } from "vitest";

import { applyTheme, resolveTheme } from "./theme";

describe("theme", () => {
  it("follows the OS only for System", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("is applied as data-theme, as tokens.css expects", () => {
    const root = document.createElement("html");
    applyTheme("light", root);
    expect(root.dataset.theme).toBe("light");
  });
});
