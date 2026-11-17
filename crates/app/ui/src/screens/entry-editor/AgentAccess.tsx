// Entry editor A's Agents section (ADR 0012): the Agent access decisions remembered for this
// Entry, one per Agent pairing and program, and a way to allow or deny a program ahead of time.
// Until Approvers arrive, a Headless host can't ask anyone, so allowing a program here is how an
// Agent gets to use the Entry with `keefast-agent run`. Decisions are kept in the Vault, not in
// the Entry, so each change applies at once and makes no Entry history.

import { useState } from "react";
import { Bot, Plus } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AgentDecisionView, EntryDetail, EntryAgentAccess } from "@/bindings";
import { Badge, Button, Input, Select, toast } from "@/kit";

export const entryAgentAccessKey = (entry: EntryDetail) => ["entryAgentAccess", entry.vault, entry.id] as const;

/** `entry` is `null` for a new Entry, which no Agent can have used yet. Shown only for an Entry
 * some Agent pairing can use, or that has decisions left. */
export function AgentAccess({ entry }: { entry: EntryDetail | null }) {
  const access = useQuery({
    queryKey: entry ? entryAgentAccessKey(entry) : ["entryAgentAccess", "new"],
    queryFn: () => api.getEntryAgentAccess(entry!.vault, entry!.id),
    enabled: entry !== null,
  });
  if (!entry || !access.data) return null;
  if (access.data.pairings.length === 0 && access.data.decisions.length === 0) return null;
  return <AgentAccessSection entry={entry} access={access.data} />;
}

export function AgentAccessSection({ entry, access }: { entry: EntryDetail; access: EntryAgentAccess }) {
  const client = useQueryClient();
  const [adding, setAdding] = useState(false);
  const refresh = () => client.invalidateQueries({ queryKey: entryAgentAccessKey(entry) });
  const run = async (change: () => Promise<unknown>) => {
    try {
      await change();
      await refresh();
      return true;
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
      return false;
    }
  };
  const remember = (pairing: string, program: string, allow: boolean) =>
    run(() => api.rememberAgentDecision(entry.vault, entry.id, pairing, program, allow));
  const forget = (decision: AgentDecisionView) =>
    run(() => api.forgetAgentDecision(entry.vault, entry.id, decision.pairing, decision.program));

  return (
    <div className="ee-sec">
      <div className="ee-sec-head">
        <h3>Agents</h3>
        <span className="n">{access.decisions.length}</span>
        {access.pairings.length > 0 && !adding && (
          <Button variant="ghost" size="sm" onClick={() => setAdding(true)}>
            <Plus /> Program
          </Button>
        )}
      </div>
      {access.decisions.length === 0 ? (
        <div className="ee-empty">
          No program is allowed or denied yet: {pairingNames(access)} can't use this Entry until you allow one.
        </div>
      ) : (
        access.decisions.map((decision) => (
          <div key={`${decision.pairing}:${decision.program}`} className="ee-att ee-agent">
            <span className="ee-att-ic">
              <Bot />
            </span>
            <span className="nm truncate">
              {decision.pairingName} <span className="subtle">with</span> <code>{decision.program}</code>
            </span>
            <Badge tone={decision.allow ? "ok" : "danger"}>{decision.allow ? "Allowed" : "Denied"}</Badge>
            <span className="ee-att-acts">
              <Button variant="ghost" size="sm" onClick={() => void remember(decision.pairing, decision.program, !decision.allow)}>
                {decision.allow ? "Deny" : "Allow"}
              </Button>
              <Button variant="dangerGhost" size="sm" onClick={() => void forget(decision)}>
                Forget
              </Button>
            </span>
          </div>
        ))
      )}
      {adding && (
        <AddDecision
          access={access}
          onCancel={() => setAdding(false)}
          onDecide={async (pairing, program, allow) => {
            if (await remember(pairing, program, allow)) setAdding(false);
          }}
        />
      )}
      <div className="ee-agent-note">
        Agents use this Entry with <code>keefast-agent run</code>. Shells and interpreters, such as bash or python, are never
        remembered: they ask every time.
      </div>
    </div>
  );
}

function pairingNames(access: EntryAgentAccess): string {
  const names = access.pairings.map((pairing) => pairing.name);
  if (names.length === 0) return "Agents";
  const last = names.pop()!;
  return names.length === 0 ? last : `${names.join(", ")} and ${last}`;
}

function AddDecision({
  access,
  onCancel,
  onDecide,
}: {
  access: EntryAgentAccess;
  onCancel: () => void;
  onDecide: (pairing: string, program: string, allow: boolean) => Promise<void>;
}) {
  const [first] = access.pairings;
  const [pairing, setPairing] = useState(first?.id ?? "");
  const [program, setProgram] = useState("");
  const decide = (allow: boolean) => {
    if (program.trim()) void onDecide(pairing, program.trim(), allow);
  };
  return (
    <div className="ee-agent-add">
      {access.pairings.length > 1 ? (
        <Select
          label="Agent"
          value={pairing}
          onValueChange={setPairing}
          options={access.pairings.map((candidate) => ({ value: candidate.id, label: candidate.name }))}
        />
      ) : (
        <span className="nm">{first?.name}</span>
      )}
      <span className="subtle">with</span>
      <Input
        aria-label="Program"
        placeholder="gh"
        autoFocus
        value={program}
        onChange={(event) => setProgram(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") decide(true);
          if (event.key === "Escape") {
            event.stopPropagation();
            onCancel();
          }
        }}
      />
      <Button size="sm" disabled={!program.trim()} onClick={() => decide(true)}>
        Allow
      </Button>
      <Button variant="ghost" size="sm" disabled={!program.trim()} onClick={() => decide(false)}>
        Deny
      </Button>
      <Button variant="ghost" size="sm" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
}
