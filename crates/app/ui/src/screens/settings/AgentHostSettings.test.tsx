import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentActivityRecord } from "@/bindings";

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    addAgentHost: vi.fn(),
    getAgentActivity: vi.fn(),
    getVaultAgentActivity: vi.fn(),
  },
}));

const { ActivityDialog, AddHostDialog } = await import("./AgentHostSettings");
const { api } = await import("@/api");

afterEach(() => {
  cleanup();
  vi.mocked(api.addAgentHost).mockReset();
});

function fill(url: string, token: string) {
  fireEvent.change(screen.getByLabelText("Address"), { target: { value: url } });
  fireEvent.change(screen.getByLabelText("Approver token"), { target: { value: token } });
}

describe("Add an Agent host", () => {
  it("adds the host by its address and Approver token, then closes", async () => {
    vi.mocked(api.addAgentHost).mockResolvedValue({
      id: "h1",
      name: "build.example:7807",
      url: "https://build.example:7807",
      trustsOwnCertificate: false,
      state: "connecting",
      problem: null,
    });
    const onClose = vi.fn();
    render(<AddHostDialog onClose={onClose} />);

    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Add" }).disabled).toBe(true);
    fill("https://build.example:7807", "kfapprover_token");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(api.addAgentHost).toHaveBeenCalledWith("", "https://build.example:7807", "kfapprover_token", null);
  });

  it("passes the certificate pasted for a host with a self-signed one", async () => {
    vi.mocked(api.addAgentHost).mockResolvedValue({
      id: "h1",
      name: "lab.example",
      url: "https://lab.example",
      trustsOwnCertificate: true,
      state: "connecting",
      problem: null,
    });
    const onClose = vi.fn();
    render(<AddHostDialog onClose={onClose} />);

    fill("https://lab.example", "kfapprover_token");
    fireEvent.change(screen.getByLabelText("Certificate"), { target: { value: "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----" } });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(api.addAgentHost).toHaveBeenCalledWith("", "https://lab.example", "kfapprover_token", "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----");
  });

  it("shows why a host was refused, and stays open", async () => {
    vi.mocked(api.addAgentHost).mockRejectedValue(new Error("That is an Agent pairing's token."));
    const onClose = vi.fn();
    render(<AddHostDialog onClose={onClose} />);

    fill("https://build.example", "kfa_agent");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(await screen.findByText("That is an Agent pairing's token.")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });
});

function record(overrides: Partial<AgentActivityRecord>): AgentActivityRecord {
  return {
    time: 1_900_000_000,
    pairing: "claude",
    kind: "agent",
    action: "use",
    entryUuid: "00".repeat(16),
    entryTitle: "GitHub",
    program: "gh",
    method: null,
    httpHost: null,
    field: null,
    outcome: "allowed",
    reason: null,
    claimedFor: null,
    ...overrides,
  };
}

const HOST = { id: "h1", name: "Build server", url: "https://build.example", trustsOwnCertificate: false, state: "connected", problem: null } as const;

function showActivity() {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <ActivityDialog host={HOST} onClose={() => {}} />
    </QueryClientProvider>,
  );
}

describe("an Agent host's Activity", () => {
  it("shows a run, an http_request, a create and a denied reveal, and filters them", async () => {
    vi.mocked(api.getAgentActivity).mockResolvedValue([
      record({ action: "reveal", entryTitle: "Wallet", program: null, field: "password", outcome: "denied", reason: "denied once at a prompt" }),
      record({ pairing: "codex", action: "create", entryTitle: "Database", program: null }),
      record({ action: "http_request", program: null, method: "GET", httpHost: "api.github.com" }),
      record({}),
    ]);
    showActivity();

    const list = await screen.findByRole("list", { name: "Activity" });
    const rows = () => within(list).getAllByRole("listitem").map((row) => row.textContent);
    expect(rows()).toHaveLength(4);
    expect(rows()[0]).toContain("Reveal password of “Wallet”");
    expect(rows()[0]).toContain("Denied");
    expect(rows()[0]).toContain("denied once at a prompt");
    expect(rows()[1]).toContain("Create “Database”");
    expect(rows()[2]).toContain("GET api.github.com with “GitHub”");
    expect(rows()[3]).toContain("Run gh with “GitHub”");
    expect(api.getAgentActivity).toHaveBeenCalledWith("h1");

    fireEvent.click(screen.getByRole("radio", { name: "Denied" }));
    expect(rows()).toHaveLength(1);
    fireEvent.click(screen.getByRole("radio", { name: "All" }));
    fireEvent.change(screen.getByLabelText("Entry"), { target: { value: "github" } });
    expect(rows()).toHaveLength(2);
  });

  it("shows a Gateway's resolve as a Gateway's, with whom it said it was for, unverified", async () => {
    vi.mocked(api.getAgentActivity).mockResolvedValue([
      record({ pairing: "agentes", kind: "gateway", action: "resolve", program: null, claimedFor: { agent: "researcher", person: "ana", run: null } }),
    ]);
    showActivity();

    const list = await screen.findByRole("list", { name: "Activity" });
    const [row] = within(list).getAllByRole("listitem");
    expect(row!.textContent).toContain("Resolve the value of “GitHub”");
    expect(row!.textContent).toContain("agentes (Gateway)");
    expect(row!.textContent).toContain("For agent researcher, person ana (claimed, unverified)");
  });

  it("says why the host's Activity can't be read", async () => {
    vi.mocked(api.getAgentActivity).mockRejectedValue(new Error("The Agent host couldn't be reached, so its Activity can't be shown."));
    showActivity();

    expect((await screen.findByRole("alert")).textContent).toContain("couldn't be reached");
  });
});

describe("a Vault's own Activity", () => {
  it("reads the Vault's log in this desktop's in-app host, not a host's", async () => {
    vi.mocked(api.getVaultAgentActivity).mockResolvedValue([record({})]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ActivityDialog vault={{ id: 1, name: "Work" }} onClose={() => {}} />
      </QueryClientProvider>,
    );

    const list = await screen.findByRole("list", { name: "Activity" });
    expect(within(list).getAllByRole("listitem")[0]!.textContent).toContain("Run gh with “GitHub”");
    expect(screen.getByText("Activity on Work")).toBeTruthy();
    expect(api.getVaultAgentActivity).toHaveBeenCalledWith(1);
  });
});
