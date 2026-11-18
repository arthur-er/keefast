import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentPairingView, AgentSettingsView } from "@/bindings";

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    createAgentPairing: vi.fn(),
    createGatewayPairing: vi.fn(),
    revokeAgentPairing: vi.fn(),
  },
}));

const { gatewaysSection, PairAgentDialog, PairGatewayDialog, readsText, gatewayText } = await import("./AgentSettings");
const { api } = await import("@/api");

const claude: AgentPairingView = { id: "p1", name: "claude", group: "claude", reads: ["shared"], canCreate: false };

const agentes: AgentPairingView = { id: "g1", name: "agentes", group: "agentes", reads: ["shared"], canCreate: true };

const settings: AgentSettingsView = {
  agentGroup: "agents",
  agentGroupName: "Agents",
  subgroups: [
    { id: "claude", name: "claude" },
    { id: "shared", name: "shared" },
    { id: "agentes", name: "agentes" },
  ],
  pairings: [claude],
  gateways: [agentes],
};

afterEach(() => {
  cleanup();
  vi.mocked(api.createAgentPairing).mockReset();
  vi.mocked(api.createGatewayPairing).mockReset();
  vi.mocked(api.revokeAgentPairing).mockReset();
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
    expect(api.createGatewayPairing).not.toHaveBeenCalled();
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
    vi.mocked(api.createAgentPairing).mockRejectedValue(new Error("This Vault already has a pairing with this name."));
    render(<PairAgentDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "claude" } });
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    await waitFor(() => expect(screen.getByText("This Vault already has a pairing with this name.")).toBeTruthy());
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

describe("Pair a Gateway", () => {
  it("pairs the Gateway with the subgroups it reads and whether it writes, then shows its token once", async () => {
    vi.mocked(api.createGatewayPairing).mockResolvedValue({
      pairing: { id: "g2", name: "chat", group: "chat", reads: ["shared"], canCreate: true },
      token: "kfa_gateway-token",
    });
    const onClose = vi.fn();
    render(<PairGatewayDialog vault={1} settings={settings} onClose={onClose} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "chat" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "shared" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Creates and changes logins in its own subgroup" }));
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    const token = await screen.findByLabelText("Token");
    expect(api.createGatewayPairing).toHaveBeenCalledWith(1, "chat", ["shared"], true);
    expect(api.createAgentPairing).not.toHaveBeenCalled();
    expect((token as HTMLInputElement).value).toBe("kfa_gateway-token");
    expect(screen.getByText(/shows it only now/)).toBeTruthy();
    expect(screen.getByText(/gateway\/v1/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onClose).toHaveBeenCalled();
  });

  it("is read-only unless asked to write", async () => {
    vi.mocked(api.createGatewayPairing).mockResolvedValue({
      pairing: { id: "g2", name: "chat", group: "chat", reads: [], canCreate: false },
      token: "kfa_gateway-token",
    });
    render(<PairGatewayDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "chat" } });
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    await screen.findByLabelText("Token");
    expect(api.createGatewayPairing).toHaveBeenCalledWith(1, "chat", [], false);
  });

  it("says why a pairing was refused, and shows no token", async () => {
    vi.mocked(api.createGatewayPairing).mockRejectedValue(new Error("This Vault already has a pairing with this name."));
    render(<PairGatewayDialog vault={1} settings={settings} onClose={() => {}} />);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "agentes" } });
    fireEvent.click(screen.getByRole("button", { name: "Pair" }));

    await waitFor(() => expect(screen.getByText("This Vault already has a pairing with this name.")).toBeTruthy());
    expect(screen.queryByLabelText("Token")).toBeNull();
  });
});

describe("the Gateways section", () => {
  function showSection(given: AgentSettingsView) {
    const section = gatewaysSection(1, given);
    render(<>{section.cards.flat().map((item) => <div key={item.id}>{item.render()}</div>)}</>);
    return section;
  }

  it("lists Gateway pairings apart from Agents, with what they read and whether they write", () => {
    const section = showSection(settings);

    expect(section.count).toBe(1);
    expect(screen.getByText("agentes")).toBeTruthy();
    expect(screen.getByText("Reads agentes, shared · writes logins")).toBeTruthy();
    expect(screen.queryByText("claude")).toBeNull();
  });

  it("says when no Gateway is paired", () => {
    showSection({ ...settings, gateways: [] });

    expect(screen.getByText(/No Gateway is paired/)).toBeTruthy();
  });

  it("revokes a Gateway once confirmed", async () => {
    vi.mocked(api.revokeAgentPairing).mockResolvedValue(null);
    showSection(settings);

    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
    expect(await screen.findByText("Revoke the Gateway “agentes”?")).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: "Revoke" }).at(-1)!);

    await waitFor(() => expect(api.revokeAgentPairing).toHaveBeenCalledWith(1, "g1"));
  });
});

describe("a Gateway pairing's row", () => {
  it("says which subgroups it reads, its own first, and whether it writes", () => {
    expect(gatewayText(agentes, settings)).toBe("Reads agentes, shared · writes logins");
    expect(gatewayText({ ...agentes, canCreate: false, reads: [] }, settings)).toBe("Reads agentes · read only");
  });
});
