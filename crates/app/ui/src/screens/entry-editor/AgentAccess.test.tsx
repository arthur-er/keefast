import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentDecisionView, EntryDetail } from "@/bindings";

// The Vault's Agent settings for one Entry, held in the test's hands.
const vault = vi.hoisted(() => ({
  pairings: [] as { id: string; name: string; group: string; reads: string[] }[],
  decisions: [] as AgentDecisionView[],
  toasts: [] as string[],
}));

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    getEntryAgentAccess: vi.fn(async () => ({ pairings: vault.pairings, decisions: [...vault.decisions] })),
    rememberAgentDecision: vi.fn(async (_vault: number, _entry: string, pairing: string, program: string, allow: boolean) => {
      if (program === "bash") throw new Error("bash is a shell or interpreter, which can run anything");
      vault.decisions = vault.decisions.filter((d) => !(d.pairing === pairing && d.program === program));
      vault.decisions.push({ pairing, pairingName: "claude", program, allow });
      return null;
    }),
    forgetAgentDecision: vi.fn(async (_vault: number, _entry: string, pairing: string, program: string) => {
      vault.decisions = vault.decisions.filter((d) => !(d.pairing === pairing && d.program === program));
      return null;
    }),
  },
}));

vi.mock("@/kit", async (original) => ({
  ...(await original<typeof import("@/kit")>()),
  toast: (message: string) => vault.toasts.push(message),
}));

const { AgentAccess } = await import("./AgentAccess");
const { api } = await import("@/api");

const entry = { vault: 1, id: "e1", title: "GitHub token" } as EntryDetail;
const claude = { id: "p1", name: "claude", group: "claude", reads: [] };

function show(shown: EntryDetail | null = entry) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AgentAccess entry={shown} />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vault.pairings = [claude];
  vault.decisions = [];
  vault.toasts = [];
});

afterEach(() => {
  cleanup();
  vi.mocked(api.rememberAgentDecision).mockClear();
  vi.mocked(api.forgetAgentDecision).mockClear();
});

describe("The Entry editor's Agents section", () => {
  it("says which Agents can't use the Entry yet, and allows a program for one", async () => {
    show();

    expect(await screen.findByText(/claude can't use this Entry until you allow one/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Program" }));
    fireEvent.change(screen.getByLabelText("Program"), { target: { value: "gh" } });
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));

    expect(await screen.findByText("Allowed")).toBeTruthy();
    expect(api.rememberAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "gh", true);
    expect(screen.getByText("gh")).toBeTruthy();
  });

  it("turns an allowed program into a denied one, and forgets it", async () => {
    vault.decisions = [{ pairing: "p1", pairingName: "claude", program: "gh", allow: true }];
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Deny" }));
    expect(await screen.findByText("Denied")).toBeTruthy();
    expect(api.rememberAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "gh", false);

    fireEvent.click(screen.getByRole("button", { name: "Forget" }));
    await waitFor(() => expect(screen.queryByText("Denied")).toBeNull());
    expect(api.forgetAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "gh");
  });

  it("shows why a shell can't be remembered and keeps the form open", async () => {
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Program" }));
    fireEvent.change(screen.getByLabelText("Program"), { target: { value: "bash" } });
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(vault.toasts).toEqual(["bash is a shell or interpreter, which can run anything"]));
    expect(screen.getByLabelText("Program")).toBeTruthy();
  });

  it("isn't shown for an Entry no Agent can use, or a new Entry", async () => {
    vault.pairings = [];
    show();
    show(null);

    await waitFor(() => expect(api.getEntryAgentAccess).toHaveBeenCalled());
    expect(screen.queryByText("Agents")).toBeNull();
  });
});
