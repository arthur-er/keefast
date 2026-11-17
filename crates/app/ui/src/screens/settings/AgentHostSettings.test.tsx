import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/api", () => ({
  errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
  api: {
    addAgentHost: vi.fn(),
  },
}));

const { AddHostDialog } = await import("./AgentHostSettings");
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
      state: "connecting",
      problem: null,
    });
    const onClose = vi.fn();
    render(<AddHostDialog onClose={onClose} />);

    expect(screen.getByRole<HTMLButtonElement>("button", { name: "Add" }).disabled).toBe(true);
    fill("https://build.example:7807", "kfapprover_token");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(api.addAgentHost).toHaveBeenCalledWith("", "https://build.example:7807", "kfapprover_token");
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
