import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => String(error),
  api: { quit: vi.fn(async () => null) },
}));

const { useQuitCommand } = await import("./quit");
const { useAppCommands } = await import("./commands");
const { api } = await import("@/api");

afterEach(() => {
  cleanup();
  vi.mocked(api.quit).mockClear();
});

describe("Quit keefast", () => {
  it("is in the command palette with its Ctrl Q shortcut", () => {
    renderHook(() => useQuitCommand(true));
    const { result } = renderHook(() => useAppCommands());
    const quit = result.current.find((command) => command.id === "quit");
    expect(quit?.title).toBe("Quit keefast");
    expect(quit?.shortcut).toEqual(["Ctrl Q"]);

    quit?.run();
    expect(api.quit).toHaveBeenCalledTimes(1);
  });

  it("quits on Ctrl Q, and not while another screen has the keys", () => {
    const { rerender } = renderHook(({ active }) => useQuitCommand(active), { initialProps: { active: false } });
    fireEvent.keyDown(window, { key: "q", ctrlKey: true });
    expect(api.quit).not.toHaveBeenCalled();

    rerender({ active: true });
    fireEvent.keyDown(window, { key: "q", ctrlKey: true });
    expect(api.quit).toHaveBeenCalledTimes(1);
  });
});
