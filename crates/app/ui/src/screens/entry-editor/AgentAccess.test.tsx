import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentDecisionView, EntryDetail } from "@/bindings";

// The Vault's Agent settings for one Entry, held in the test's hands.
const vault = vi.hoisted(() => ({
  pairings: [] as { id: string; name: string; group: string; reads: string[] }[],
  decisions: [] as AgentDecisionView[],
  allowedHosts: [] as string[],
  agentsMayReveal: false,
  toasts: [] as string[],
}));

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    getEntryAgentAccess: vi.fn(async () => ({
      pairings: vault.pairings,
      decisions: [...vault.decisions],
      allowedHosts: [...vault.allowedHosts],
      agentsMayReveal: vault.agentsMayReveal,
    })),
    setEntryAgentsMayReveal: vi.fn(async (_vault: number, _entry: string, mayReveal: boolean) => {
      vault.agentsMayReveal = mayReveal;
      return null;
    }),
    rememberAgentDecision: vi.fn(
      async (_vault: number, _entry: string, pairing: string, kind: "program" | "host", name: string, allow: boolean) => {
        if (name === "bash") throw new Error("bash is a shell or interpreter, which can run anything");
        vault.decisions = vault.decisions.filter((d) => !(d.pairing === pairing && d.kind === kind && d.name === name));
        vault.decisions.push({ pairing, pairingName: "claude", kind, name, allow });
        return null;
      },
    ),
    forgetAgentDecision: vi.fn(async (_vault: number, _entry: string, pairing: string, kind: string, name: string) => {
      vault.decisions = vault.decisions.filter((d) => !(d.pairing === pairing && d.kind === kind && d.name === name));
      return null;
    }),
    setEntryAllowedHosts: vi.fn(async (_vault: number, _entry: string, hosts: string[]) => {
      const bad = hosts.find((host) => host.includes(":"));
      if (bad) throw new Error(`${bad} isn't a host`);
      vault.allowedHosts = hosts.map((host) => host.toLowerCase());
      return vault.allowedHosts;
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
const claude = { id: "p1", name: "claude", group: "claude", reads: [], canCreate: false };

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
  vault.allowedHosts = [];
  vault.agentsMayReveal = false;
  vault.toasts = [];
});

afterEach(() => {
  cleanup();
  vi.mocked(api.rememberAgentDecision).mockClear();
  vi.mocked(api.forgetAgentDecision).mockClear();
  vi.mocked(api.setEntryAllowedHosts).mockClear();
});

describe("The Entry editor's Agents section", () => {
  it("says which Agents can't use the Entry yet, and allows a program for one", async () => {
    show();

    expect(await screen.findByText(/claude can't use this Entry until you allow one/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Program" }));
    fireEvent.change(screen.getByLabelText("Program"), { target: { value: "gh" } });
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));

    expect(await screen.findByText("Allowed")).toBeTruthy();
    expect(api.rememberAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "program", "gh", true);
    expect(screen.getByText("gh")).toBeTruthy();
  });

  it("turns an allowed program into a denied one, and forgets it", async () => {
    vault.decisions = [{ pairing: "p1", pairingName: "claude", kind: "program", name: "gh", allow: true }];
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Deny" }));
    expect(await screen.findByText("Denied")).toBeTruthy();
    expect(api.rememberAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "program", "gh", false);

    fireEvent.click(screen.getByRole("button", { name: "Forget" }));
    await waitFor(() => expect(screen.queryByText("Denied")).toBeNull());
    expect(api.forgetAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "program", "gh");
  });

  it("shows why a shell can't be remembered and keeps the form open", async () => {
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Program" }));
    fireEvent.change(screen.getByLabelText("Program"), { target: { value: "bash" } });
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));

    await waitFor(() => expect(vault.toasts).toEqual(["bash is a shell or interpreter, which can run anything"]));
    expect(screen.getByLabelText("Program")).toBeTruthy();
  });

  it("adds and removes the hosts http_request may send the Entry's secrets to", async () => {
    show();

    expect(await screen.findByText(/No host yet/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Allowed host"), { target: { value: "API.GitHub.com" } });
    fireEvent.keyDown(screen.getByLabelText("Allowed host"), { key: "Enter" });
    expect(await screen.findByText("api.github.com")).toBeTruthy();
    expect(api.setEntryAllowedHosts).toHaveBeenLastCalledWith(1, "e1", ["API.GitHub.com"]);

    fireEvent.change(screen.getByLabelText("Allowed host"), { target: { value: "uploads.github.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByText("uploads.github.com")).toBeTruthy();
    expect(api.setEntryAllowedHosts).toHaveBeenLastCalledWith(1, "e1", ["api.github.com", "uploads.github.com"]);

    fireEvent.click(screen.getByRole("button", { name: "Remove api.github.com" }));
    await waitFor(() => expect(screen.queryByText("api.github.com")).toBeNull());
    expect(api.setEntryAllowedHosts).toHaveBeenLastCalledWith(1, "e1", ["uploads.github.com"]);
  });

  it("keeps a host that isn't one in the field and says why", async () => {
    show();

    fireEvent.change(await screen.findByLabelText("Allowed host"), { target: { value: "api.github.com:443" } });
    fireEvent.keyDown(screen.getByLabelText("Allowed host"), { key: "Enter" });

    await waitFor(() => expect(vault.toasts).toEqual(["api.github.com:443 isn't a host"]));
    expect((screen.getByLabelText("Allowed host") as HTMLInputElement).value).toBe("api.github.com:443");
  });

  it("allows a host ahead of time, and shows it apart from programs", async () => {
    vault.decisions = [{ pairing: "p1", pairingName: "claude", kind: "program", name: "gh", allow: true }];
    show();

    fireEvent.click(await screen.findByRole("button", { name: "Host" }));
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "api.github.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));

    expect(await screen.findByText("api.github.com")).toBeTruthy();
    expect(api.rememberAgentDecision).toHaveBeenCalledWith(1, "e1", "p1", "host", "api.github.com", true);
    expect(screen.getByText("at")).toBeTruthy();
    expect(screen.getByText("with")).toBeTruthy();
  });

  it("isn't shown for an Entry no Agent can use, or a new Entry", async () => {
    vault.pairings = [];
    show();
    show(null);

    await waitFor(() => expect(api.getEntryAgentAccess).toHaveBeenCalled());
    expect(screen.queryByText("Agents")).toBeNull();
  });

  it("marks the Entry Agents may reveal, saying its value then reaches the model when allowed", async () => {
    show();

    const mark = await screen.findByRole("switch", { name: "Agents may reveal" });
    expect(mark.getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText(/the value itself reaches the model/)).toBeTruthy();
    fireEvent.click(mark);

    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Agents may reveal" }).getAttribute("aria-checked")).toBe("true"),
    );
    expect(api.setEntryAgentsMayReveal).toHaveBeenCalledWith(1, "e1", true);
  });

  it("shows a marked Entry's section even when no Agent can read it any more, to take the mark off", async () => {
    vault.pairings = [];
    vault.agentsMayReveal = true;
    show();

    const mark = await screen.findByRole("switch", { name: "Agents may reveal" });
    fireEvent.click(mark);

    await waitFor(() => expect(api.setEntryAgentsMayReveal).toHaveBeenCalledWith(1, "e1", false));
  });
});
