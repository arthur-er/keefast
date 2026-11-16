import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The OS registration, held in the test's hands.
const os = vi.hoisted(() => ({ registered: false, failure: null as Error | null }));

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  onEvent: () => () => {},
  api: {
    getStartOnBoot: vi.fn(async () => os.registered),
    setStartOnBoot: vi.fn(async (enabled: boolean) => {
      if (os.failure) throw os.failure;
      os.registered = enabled;
      return null;
    }),
  },
}));

const { StartOnBootRow, START_ON_BOOT_SEARCH } = await import("./StartOnBoot");
const { searchSections } = await import("./search");
const { api } = await import("@/api");

function showRow() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <StartOnBootRow />
    </QueryClientProvider>,
  );
  return screen.getByRole("switch", { name: "Start on boot" });
}

beforeEach(() => {
  os.registered = false;
  os.failure = null;
  vi.mocked(api.getStartOnBoot).mockClear();
  vi.mocked(api.setStartOnBoot).mockClear();
});

afterEach(cleanup);

describe("Start on boot", () => {
  it("shows what the OS has registered", async () => {
    os.registered = true;
    const toggle = showRow();
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  });

  it("registers keefast and reads the OS back", async () => {
    const toggle = showRow();
    await waitFor(() => expect(toggle.hasAttribute("disabled")).toBe(false));
    expect(toggle.getAttribute("aria-checked")).toBe("false");

    fireEvent.click(toggle);

    expect(api.setStartOnBoot).toHaveBeenCalledWith(true);
    await waitFor(() => expect(api.getStartOnBoot).toHaveBeenCalledTimes(2));
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    expect(os.registered).toBe(true);
  });

  it("goes back to what the OS has when registering fails", async () => {
    os.failure = new Error("keefast couldn't set itself to start on boot.");
    const toggle = showRow();
    await waitFor(() => expect(toggle.hasAttribute("disabled")).toBe(false));

    fireEvent.click(toggle);

    await waitFor(() => expect(api.getStartOnBoot).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
  });

  it("is found by the settings search", () => {
    const sections = [{ id: "window", name: "Window & startup", cards: [[{ id: "startOnBoot", search: START_ON_BOOT_SEARCH }]] }];
    for (const query of ["start on boot", "login", "autostart", "background"]) {
      expect(searchSections(sections, query).sections[0]!.count).toBe(1);
    }
    expect(searchSections(sections, "clipboard").empty).toBe(true);
  });
});
