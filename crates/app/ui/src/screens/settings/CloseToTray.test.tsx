import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CLOSE_TO_TRAY_DESC, CLOSE_TO_TRAY_SEARCH, CloseToTrayRow, NO_TRAY_DESC } from "./CloseToTray";
import { searchSections } from "./search";

afterEach(cleanup);

describe("Close to tray", () => {
  it("is off until turned on, and turns on", () => {
    const onChange = vi.fn();
    render(<CloseToTrayRow enabled={false} trayAvailable onChange={onChange} />);
    const toggle = screen.getByRole("switch", { name: "Close to tray" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText(CLOSE_TO_TRAY_DESC)).toBeTruthy();

    fireEvent.click(toggle);

    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("says closing will minimize where the desktop has no tray", () => {
    render(<CloseToTrayRow enabled trayAvailable={false} onChange={() => {}} />);
    expect(screen.getByText("No system tray found; closing will minimize")).toBeTruthy();
    expect(screen.queryByText(CLOSE_TO_TRAY_DESC)).toBeNull();
    expect(screen.getByRole("switch", { name: "Close to tray" }).getAttribute("aria-checked")).toBe("true");
  });

  it("is found by the settings search", () => {
    const sections = [{ id: "window", name: "Window & startup", cards: [[{ id: "closeToTray", search: CLOSE_TO_TRAY_SEARCH }]] }];
    for (const query of ["close to tray", "system tray", "minimize", NO_TRAY_DESC.slice(0, 15)]) {
      expect(searchSections(sections, query).sections[0]!.count).toBe(1);
    }
    expect(searchSections(sections, "clipboard").empty).toBe(true);
  });
});
