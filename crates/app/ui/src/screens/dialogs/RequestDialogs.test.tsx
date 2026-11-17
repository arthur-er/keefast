import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceSnapshot } from "@/bindings";

// The backend's answers aren't what is tested: they never come, and the answers sent are counted.
vi.mock("@/api", () => ({
  api: {
    answerAccess: vi.fn(() => new Promise(() => {})),
    answerAgentPrompt: vi.fn(() => new Promise(() => {})),
    approvePairing: vi.fn(() => new Promise(() => {})),
    denyPairing: vi.fn(() => new Promise(() => {})),
    getEntry: () => new Promise(() => {}),
    getVaultSettings: () => new Promise(() => {}),
    getWorkspace: () => new Promise(() => {}),
  },
  errorMessage: String,
}));
vi.mock("@/shell/state", () => ({
  useShell: () => ({ state: { overlay: null }, actions: { closeOverlay: () => {} } }),
}));

const { ARM_DELAY_MS, RequestDialogs } = await import("./RequestDialogs");
const { api } = await import("@/api");

const NOW = Date.parse("2026-10-29T12:00:00Z");

function snapshot(pending: Partial<WorkspaceSnapshot>): WorkspaceSnapshot {
  return {
    revision: 1,
    vaults: [],
    view: { scope: { kind: "allVaults" }, entry: null },
    saveStatus: "saved",
    vaultToUnlock: null,
    recentVaults: [],
    tags: [],
    pendingLock: null,
    pendingPairing: null,
    pendingAccess: null,
    pendingLogin: null,
    pendingAgentPrompt: null,
    clipboardClearsIn: null,
    dropboxAvailable: false,
    configError: null,
    ...pending,
  };
}

const access = (changes: Partial<NonNullable<WorkspaceSnapshot["pendingAccess"]>> = {}) =>
  snapshot({
    pendingAccess: {
      id: 7,
      site: "github.com",
      origin: "https://github.com",
      formSite: null,
      formOrigin: null,
      entries: [{ vault: 0, entry: "e1", title: "GitHub", username: "arthur-er", vaultName: "Personal" }],
      pairings: ["Chrome on my laptop"],
      asked: NOW / 1000,
      ...changes,
    },
  });

const agent = (changes: Partial<NonNullable<WorkspaceSnapshot["pendingAgentPrompt"]>> = {}) =>
  snapshot({
    pendingAgentPrompt: {
      id: "5f1c",
      host: "h1",
      hostName: "Build server",
      pairing: "claude",
      entryTitle: "deploy token",
      reference: "claude/GitHub/deploy token#password",
      program: "gh",
      command: ["gh", "pr", "create", "--title", "Fix the login"],
      canRemember: true,
      expires: NOW / 1000 + 60,
      more: 0,
      ...changes,
    },
  });

const pairing = snapshot({ pendingPairing: { id: 3, vault: 0, vaultName: "Personal" } });

function show(shown: WorkspaceSnapshot) {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <RequestDialogs snapshot={shown} />
    </QueryClientProvider>,
  );
}

const button = (name: RegExp) => screen.getByRole<HTMLButtonElement>("button", { name });

beforeEach(() => {
  vi.useFakeTimers({ now: NOW });
  vi.mocked(api.answerAccess).mockClear();
  vi.mocked(api.answerAgentPrompt).mockClear();
  vi.mocked(api.approvePairing).mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Allow", () => {
  it("can't be pressed until the dialog has shown for a moment", () => {
    show(access());

    expect(button(/^Allow/).disabled).toBe(true);
    fireEvent.click(button(/^Allow/));
    act(() => vi.advanceTimersByTime(ARM_DELAY_MS - 1));
    expect(button(/^Allow/).disabled).toBe(true);
    expect(api.answerAccess).not.toHaveBeenCalled();

    act(() => vi.advanceTimersByTime(1));
    expect(button(/^Allow/).disabled).toBe(false);
    fireEvent.click(button(/^Allow/));
    expect(api.answerAccess).toHaveBeenCalledWith(7, true, false);
  });

  it("waits again for the next request", () => {
    const { rerender } = show(access());
    act(() => vi.advanceTimersByTime(ARM_DELAY_MS));
    expect(button(/^Allow/).disabled).toBe(false);

    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <RequestDialogs snapshot={access({ id: 8 })} />
      </QueryClientProvider>,
    );

    expect(button(/^Allow/).disabled).toBe(true);
  });

  it("leaves Deny pressable at once", () => {
    show(access());

    expect(button(/^Deny/).disabled).toBe(false);
    fireEvent.click(button(/^Deny/));
    expect(api.answerAccess).toHaveBeenCalledWith(7, false, false);
  });
});

describe("Pair", () => {
  it("can't be pressed, nor Enter pair, until the dialog has shown for a moment", () => {
    show(pairing);
    const name = screen.getByLabelText("Browser name");
    fireEvent.change(name, { target: { value: "Firefox at work" } });

    expect(button(/^Pair/).disabled).toBe(true);
    fireEvent.keyDown(name, { key: "Enter" });
    act(() => vi.advanceTimersByTime(ARM_DELAY_MS - 1));
    expect(api.approvePairing).not.toHaveBeenCalled();

    act(() => vi.advanceTimersByTime(1));
    expect(button(/^Pair/).disabled).toBe(false);
    fireEvent.keyDown(name, { key: "Enter" });
    expect(api.approvePairing).toHaveBeenCalledWith(3, "Firefox at work");
  });
});

describe("the Access prompt", () => {
  it("says which browser asked, and how long ago", () => {
    show(access());
    const kicker = () => document.querySelector(".dg-kicker")!.textContent;

    expect(kicker()).toBe("Chrome on my laptopBrowser requestjust now");
    act(() => vi.advanceTimersByTime(3 * 60_000));
    expect(kicker()).toBe("Chrome on my laptopBrowser request3 min ago");
  });

  it("names no browser when none of its Pairings is open", () => {
    show(access({ pairings: [] }));

    expect(document.querySelector(".dg-kicker")!.textContent).toBe("Browser requestjust now");
  });

  it("shows the full addresses, their schemes muted, an unencrypted one with an open lock", () => {
    show(access({ formSite: "collect.example.net", formOrigin: "http://collect.example.net" }));
    const addresses = [...document.querySelectorAll(".dg-addr")];

    expect(addresses.map((address) => address.querySelector(".host")!.textContent)).toEqual([
      "https://github.com",
      "http://collect.example.net",
    ]);
    expect(addresses.map((address) => address.querySelector(".scheme")!.textContent)).toEqual(["https://", "http://"]);
    expect(addresses.map((address) => address.querySelector(".lockico")!.classList.contains("insecure"))).toEqual([false, true]);
  });
});

describe("an Agent host's prompt", () => {
  it("names the pairing, the host, the Entry and the whole command line", () => {
    show(agent());

    expect(screen.getByRole("alertdialog", { name: "Let claude use “deploy token”?" })).toBeTruthy();
    expect(screen.getByText("claude on Build server")).toBeTruthy();
    expect(screen.getByText("claude/GitHub/deploy token#password")).toBeTruthy();
    expect(screen.getByText("gh pr create --title 'Fix the login'")).toBeTruthy();
  });

  it("allows only once armed, remembered for the program if asked", () => {
    show(agent());

    fireEvent.click(screen.getByRole("checkbox", { name: /Remember this decision for gh/ }));
    fireEvent.click(button(/^Allow/));
    expect(api.answerAgentPrompt).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(ARM_DELAY_MS));
    fireEvent.click(button(/^Allow/));

    expect(api.answerAgentPrompt).toHaveBeenCalledWith("h1", "5f1c", true, true);
  });

  it("denies with Escape", () => {
    show(agent());

    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

    expect(api.answerAgentPrompt).toHaveBeenCalledWith("h1", "5f1c", false, false);
  });

  it("offers no remembering for a shell, which asks every time", () => {
    show(agent({ program: "bash", command: ["bash", "-c", "gh auth status"], canRemember: false }));

    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.getByText(/can run anything, so keefast asks every time/)).toBeTruthy();
  });

  it("counts down to when the host denies it", () => {
    show(agent());
    expect(screen.getByText(/Denied in 60 s/)).toBeTruthy();

    act(() => vi.advanceTimersByTime(15_000));

    expect(screen.getByText(/Denied in 45 s/)).toBeTruthy();
  });
});
