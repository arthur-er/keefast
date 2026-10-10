import { describe, expect, it } from "vitest";

import { type ShellState, shellReducer } from "./state";

const base: ShellState = {
  page: { kind: "settings" },
  detail: "edit",
  overlay: null,
  openVault: "unlock",
  search: "",
  sort: "title",
  merge: null,
  lastVault: null,
  renamingGroup: null,
};

describe("shellReducer", () => {
  it("goes back to the Entry's view on another page, and leaves Open a Vault", () => {
    const next = shellReducer(base, { type: "page", page: { kind: "browser" } });
    expect(next).toMatchObject({ page: { kind: "browser" }, detail: "view", openVault: null });
  });

  it("shows the browser when the detail mode changes", () => {
    expect(shellReducer(base, { type: "detail", mode: "history" })).toMatchObject({ page: { kind: "browser" }, detail: "history" });
  });

  it("toggles the palette", () => {
    const open = shellReducer(base, { type: "togglePalette" });
    expect(open.overlay).toEqual({ kind: "palette" });
    expect(shellReducer(open, { type: "togglePalette" }).overlay).toBeNull();
  });
});
