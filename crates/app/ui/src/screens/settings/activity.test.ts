import { describe, expect, it } from "vitest";

import type { AgentActivityRecord } from "@/bindings";

import { claimText, describeActivity, filterActivity, NO_FILTER, pairingsIn } from "./activity";

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

const RUN = record({});
const HTTP = record({ action: "http_request", program: null, method: "GET", httpHost: "api.github.com" });
const CREATE = record({ pairing: "codex", action: "create", entryTitle: "Database", program: null });
const REVEAL = record({ action: "reveal", entryTitle: "Wallet", program: null, field: "password", outcome: "denied", reason: "denied once at a prompt" });
const ALL = [RUN, HTTP, CREATE, REVEAL];

describe("the Activity view", () => {
  it("says what each kind of request asked for", () => {
    expect(ALL.map(describeActivity)).toEqual([
      "Run gh with “GitHub”",
      "GET api.github.com with “GitHub”",
      "Create “Database”",
      "Reveal password of “Wallet”",
    ]);
    expect(describeActivity(record({ action: "list", entryTitle: null, program: null }))).toBe("List its secrets");
    expect(describeActivity(record({ action: "store", entryTitle: "Database", program: null, field: "Stripe key" }))).toBe("Store Stripe key in “Database”");
    expect(describeActivity(record({ action: "other", entryTitle: "Database", program: null }))).toBe("Something with “Database”");
  });

  it("says what a Gateway did with an Entry", () => {
    const gateway = { pairing: "agentes", kind: "gateway", program: null } as const;
    expect(describeActivity(record({ ...gateway, action: "resolve" }))).toBe("Resolve the value of “GitHub”");
    expect(describeActivity(record({ ...gateway, action: "create_login" }))).toBe("Create the login “GitHub”");
    expect(describeActivity(record({ ...gateway, action: "update_login" }))).toBe("Change the login “GitHub”");
    expect(describeActivity(record({ ...gateway, action: "resolve", entryTitle: null, outcome: "denied", reason: "not found" }))).toBe("Resolve the value of an Entry");
  });

  it("marks whom a Gateway said it was for as its unverified claim", () => {
    expect(claimText(record({ claimedFor: { agent: "researcher", person: "ana", run: "r-42" } }))).toBe(
      "For agent researcher, person ana, run r-42 (claimed, unverified)",
    );
    expect(claimText(record({ claimedFor: { agent: null, person: "ana", run: null } }))).toBe("For person ana (claimed, unverified)");
    expect(claimText(record({}))).toBeNull();
  });

  it("filters by pairing, Entry and outcome together", () => {
    expect(filterActivity(ALL, NO_FILTER)).toEqual(ALL);
    expect(filterActivity(ALL, { ...NO_FILTER, pairing: "codex" })).toEqual([CREATE]);
    expect(filterActivity(ALL, { ...NO_FILTER, entry: "git" })).toEqual([RUN, HTTP]);
    expect(filterActivity(ALL, { ...NO_FILTER, outcome: "denied" })).toEqual([REVEAL]);
    expect(filterActivity(ALL, { pairing: "claude", entry: "wallet", outcome: "allowed" })).toEqual([]);
  });

  it("offers each pairing the records name once", () => {
    expect(pairingsIn(ALL)).toEqual(["claude", "codex"]);
  });
});
