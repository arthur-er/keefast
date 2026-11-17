import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentPairingView, AgentSettingsView } from "@/bindings";

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    createAgentPairing: vi.fn(),
  },
}));

const { PairAgentDialog, readsText } = await import("./AgentSettings");
const { api } = await import("@/api");

const claude: AgentPairingView = { id: "p1", name: "claude", group: "claude", reads: ["shared"], canCreate: false };

const settings: AgentSettingsView = {
  agentGroup: "agents",
  agentGroupName: "Agents",
  subgroups: [
    { id: "claude", name: "claude" },
    { id: "shared", name: "shared" },
  ],
  pairings: [claude],
};

afterEach(() => {
  cleanup();
  vi.mocked(api.createAgentPairing).mockReset();
});

describe("Pair an Agent", () => {
  it("pairs the Agent with the subgroups it also reads, then shows its token once", async () => {
    vi.mocked(api.createAgentPairing).mockResolvedValue({
      pairing: { id: "p2", name: "codex", group: "codex", reads: ["shared"], canCreate: false },
      token: "kfa_secret-token",
    });
    const onClose = vi.fn();
    render(<PairAgentDialog vault={1} settings={settings} onClose={onClose} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "codex" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "shared" }));
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    const token = await screen.findByLabelText("Token");
    expect(api.createAgentPairing).toHaveBeenCalledWith(1, "codex", ["shared"], false);
    expect((token as HTMLInputElement).value).toBe("kfa_secret-token");
    expect(screen.getByText(/shows it only now/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("lets the Agent create Entries in its own subgroup when asked to", async () => {
    vi.mocked(api.createAgentPairing).mockResolvedValue({
      pairing: { id: "p2", name: "codex", group: "codex", reads: [], canCreate: true },
      token: "kfa_secret-token",
    });
    render(<PairAgentDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "codex" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Creates Entries in its own subgroup" }));
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    await screen.findByLabelText("Token");
    expect(api.createAgentPairing).toHaveBeenCalledWith(1, "codex", [], true);
  });

  it("doesn't offer an Agent its own subgroup to read as well", () => {
    render(<PairAgentDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "claude" } });

    expect(screen.queryByRole("checkbox", { name: "claude" })).toBeNull();
    expect(screen.getByRole("checkbox", { name: "shared" })).toBeTruthy();
  });

  it("says why a pairing was refused, and shows no token", async () => {
    vi.mocked(api.createAgentPairing).mockRejectedValue(new Error("This Vault already has an Agent with this name."));
    render(<PairAgentDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "claude" } });
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    await waitFor(() => expect(screen.getByText("This Vault already has an Agent with this name.")).toBeTruthy());
    expect(screen.queryByLabelText("Token")).toBeNull();
  });
});

describe("an Agent pairing's row", () => {
  it("says which subgroups it reads, its own first, and whether it creates Entries", () => {
    expect(readsText(claude, settings)).toBe("Reads claude, shared");
    expect(readsText({ ...claude, reads: [] }, settings)).toBe("Reads claude");
    expect(readsText({ ...claude, canCreate: true }, settings)).toBe("Reads claude, shared · creates Entries");
  });
});
