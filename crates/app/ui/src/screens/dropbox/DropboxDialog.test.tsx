import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { DropboxAccountView } from "@/bindings";

// The backend, held in the test's hands: connecting another account answers when the test says.
const backend = vi.hoisted(() => ({
  connecting: null as { connect: (account: DropboxAccountView) => void; fail: (error: Error) => void } | null,
  folders: [] as string[],
}));

vi.mock("@/api", () => {
  class ApiError extends Error {
    constructor(
      message: string,
      readonly kind: string,
    ) {
      super(message);
    }
  }
  return {
    ApiError,
    errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
    api: {
      dropboxStatus: async () => ({ available: true, account: { name: "Arthur", email: "arthur@example.com" }, connecting: false }),
      dropboxListFolder: async (folder: string) => {
        backend.folders.push(folder);
        return [];
      },
      dropboxConnect: vi.fn(),
      dropboxConnectAnother: () =>
        new Promise((connect, fail) => {
          backend.connecting = { connect, fail };
        }),
      dropboxCancelConnect: async () => null,
    },
  };
});

const { DropboxDialog } = await import("./DropboxDialog");
const { ApiError } = (await import("@/api")) as unknown as { ApiError: new (message: string, kind: string) => Error };

async function showConnected() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DropboxDialog onPick={() => {}} onClose={() => {}} />
    </QueryClientProvider>,
  );
  return screen.findByRole("button", { name: /Connected as arthur@example.com/ });
}

/** Opens the account menu, as the keyboard does, and picks "Use another account…". */
async function useAnotherAccount(menu: HTMLElement) {
  fireEvent.keyDown(menu, { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: /Use another account/ }));
  await screen.findByText("Waiting for Dropbox");
}

beforeEach(() => {
  backend.connecting = null;
  backend.folders = [];
});

afterEach(cleanup);

describe("Use another account…", () => {
  it("connects the account the user signs in as, from the top of its Dropbox", async () => {
    await useAnotherAccount(await showConnected());

    await act(async () => backend.connecting!.connect({ name: "Family", email: "family@example.com" }));

    expect(await screen.findByRole("button", { name: /Connected as family@example.com/ })).toBeTruthy();
    expect(backend.folders.at(-1)).toBe("");
  });

  it("goes back to the account from before when the user stops", async () => {
    await useAnotherAccount(await showConnected());

    await act(async () => backend.connecting!.fail(new ApiError("Connecting Dropbox was stopped.", "cancelled")));

    expect(await screen.findByRole("button", { name: /Connected as arthur@example.com/ })).toBeTruthy();
    expect(screen.queryByText("Connect your Dropbox")).toBeNull();
  });
});
