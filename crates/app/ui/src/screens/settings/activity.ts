// What the Activity view shows of a Headless host's audit log (ADR 0012 "Audit"): each record
// as a sentence, with whom a Gateway said it was for (ADR 0015), and the filters by pairing,
// Entry and outcome.

import type { AgentActivityRecord } from "@/bindings";

export type OutcomeFilter = "all" | "allowed" | "denied";

export interface ActivityFilter {
  /** An Agent pairing's name, or "" for all. */
  pairing: string;
  /** Text an Entry's title contains, any case; "" for all. */
  entry: string;
  outcome: OutcomeFilter;
}

export const NO_FILTER: ActivityFilter = { pairing: "", entry: "", outcome: "all" };

export function filterActivity(records: readonly AgentActivityRecord[], filter: ActivityFilter): AgentActivityRecord[] {
  const entry = filter.entry.trim().toLowerCase();
  return records.filter(
    (record) =>
      (filter.pairing === "" || record.pairing === filter.pairing) &&
      (filter.outcome === "all" || record.outcome === filter.outcome) &&
      (entry === "" || (record.entryTitle ?? "").toLowerCase().includes(entry)),
  );
}

/** The pairings the records name, sorted, for the pairing filter. */
export function pairingsIn(records: readonly AgentActivityRecord[]): string[] {
  return [...new Set(records.map((record) => record.pairing))].sort((a, b) => a.localeCompare(b));
}

/** What the Agent asked to do, as a short phrase; the outcome says whether it could. */
export function describeActivity(record: AgentActivityRecord): string {
  const entry = record.entryTitle ? `“${record.entryTitle}”` : "an Entry";
  switch (record.action) {
    case "list":
      return "List its secrets";
    case "use":
      return `Run ${record.program ?? "a program"} with ${entry}`;
    case "http_request":
      return `${record.method ?? "Send"} ${record.httpHost ?? "a host"} with ${entry}`;
    case "create":
      return `Create ${entry}`;
    case "store":
      return `Store ${record.field ?? "a value"} in ${entry}`;
    case "reveal":
      return `Reveal ${record.field ?? "a value"} of ${entry}`;
    case "resolve":
      return `Resolve the value of ${entry}`;
    case "create_login":
      return `Create the login ${entry}`;
    case "update_login":
      return `Change the login ${entry}`;
    case "other":
      return record.entryTitle ? `Something with ${entry}` : "Something keefast doesn't know";
  }
}

/** Whom a Gateway said the request was for, as `keefast-agent log` shows it: its own claim,
 * which keefast can't verify. Null when it named nobody. */
export function claimText(record: AgentActivityRecord): string | null {
  const claim = record.claimedFor;
  if (!claim) return null;
  const parts = (["agent", "person", "run"] as const).flatMap((part) => (claim[part] ? [`${part} ${claim[part]}`] : []));
  return parts.length > 0 ? `For ${parts.join(", ")} (claimed, unverified)` : null;
}
