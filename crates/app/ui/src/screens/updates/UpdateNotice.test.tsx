import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UpdateState } from "@/bindings";

const backend = vi.hoisted(() => ({
  checkForUpdates: null as boolean | null,
  updates: { currentVersion: "0.1.0", installsUpdates: true, status: { kind: "unchecked" } } as UpdateState,
}));

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  onEvent: () => () => {},
  api: {
    getSettings: vi.fn(async () => ({ checkForUpdates: backend.checkForUpdates })),
    getUpdates: vi.fn(async () => backend.updates),
    setCheckForUpdates: vi.fn(async (enabled: boolean) => {
      backend.checkForUpdates = enabled;
      return null;
    }),
    installUpdate: vi.fn(async () => null),
    openReleasePage: vi.fn(async () => null),
  },
}));

const { UpdateNotice } = await import("./UpdateNotice");
const { api } = await import("@/api");

const update = { version: "0.2.0", releasePage: "https://github.com/arthur-er/keefast/releases/tag/v0.2.0" };

function show() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <UpdateNotice />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  backend.checkForUpdates = null;
  backend.updates = { currentVersion: "0.1.0", installsUpdates: true, status: { kind: "unchecked" } };
  vi.clearAllMocks();
});

afterEach(cleanup);

describe("the update notice", () => {
  it("asks once, and keeps the answer", async () => {
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Check daily" }));

    expect(api.setCheckForUpdates).toHaveBeenCalledWith(true);
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("can be told not to check", async () => {
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Don't check" }));

    expect(api.setCheckForUpdates).toHaveBeenCalledWith(false);
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("installs an update found when asked", async () => {
    backend.checkForUpdates = true;
    backend.updates.status = { kind: "available", update };
    show();

    expect(await screen.findByText("keefast 0.2.0 is available")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Update and restart" }));

    expect(api.installUpdate).toHaveBeenCalled();
  });

  it("points an install that can't update itself at the release", async () => {
    backend.checkForUpdates = true;
    backend.updates = { ...backend.updates, installsUpdates: false, status: { kind: "available", update } };
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Download" }));

    expect(api.openReleasePage).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Update and restart" })).toBeNull();
  });

  it("stays closed for the version dismissed", async () => {
    backend.checkForUpdates = true;
    backend.updates.status = { kind: "available", update: { ...update, version: "0.9.0" } };
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status")).toBeNull();

    cleanup();
    show();
    // The query resolves, and the notice still doesn't come back.
    await waitFor(() => expect(api.getUpdates).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("status")).toBeNull();
  });
});
