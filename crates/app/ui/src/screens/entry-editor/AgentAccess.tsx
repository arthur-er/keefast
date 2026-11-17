// Entry editor A's Agents section (ADR 0012): the Agent access decisions remembered for this
// Entry, one per Agent pairing and program (`keefast-agent run`) or host (`http_request`), and a
// way to allow or deny one ahead of time, so a Headless host doesn't have to ask an Approver.
// Answers remembered at an Approver's prompt show here too. Below them, the hosts
// `http_request` may send this Entry's secrets to at all: none until the user adds one. Both are
// kept in the Vault, not in the Entry, so each change applies at once and makes no Entry history.

import { useState } from "react";
import { Bot, Globe, Plus, X } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AgentDecisionView, AgentUseKind, EntryDetail, EntryAgentAccess } from "@/bindings";
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
  const { pairings, decisions, allowedHosts } = access.data;
  if (pairings.length === 0 && decisions.length === 0 && allowedHosts.length === 0) return null;
  return <AgentAccessSection entry={entry} access={access.data} />;
}

export function AgentAccessSection({ entry, access }: { entry: EntryDetail; access: EntryAgentAccess }) {
  const client = useQueryClient();
  const [adding, setAdding] = useState<AgentUseKind | null>(null);
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
  const remember = (pairing: string, kind: AgentUseKind, name: string, allow: boolean) =>
    run(() => api.rememberAgentDecision(entry.vault, entry.id, pairing, kind, name, allow));
  const forget = (decision: AgentDecisionView) =>
    run(() => api.forgetAgentDecision(entry.vault, entry.id, decision.pairing, decision.kind, decision.name));
  const setHosts = (hosts: string[]) => run(() => api.setEntryAllowedHosts(entry.vault, entry.id, hosts));

  return (
    <div className="ee-sec">
      <div className="ee-sec-head">
        <h3>Agents</h3>
        <span className="n">{access.decisions.length}</span>
        {access.pairings.length > 0 && !adding && (
          <>
            <Button variant="ghost" size="sm" onClick={() => setAdding("program")}>
              <Plus /> Program
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setAdding("host")}>
              <Plus /> Host
            </Button>
          </>
        )}
      </div>
      {access.decisions.length === 0 ? (
        <div className="ee-empty">
          No program or host is allowed or denied yet: {pairingNames(access)} can't use this Entry until you allow one.
        </div>
      ) : (
        access.decisions.map((decision) => (
          <div key={`${decision.pairing}:${decision.kind}:${decision.name}`} className="ee-att ee-agent">
            <span className="ee-att-ic">{decision.kind === "host" ? <Globe /> : <Bot />}</span>
            <span className="nm truncate">
              {decision.pairingName} <span className="subtle">{decision.kind === "host" ? "at" : "with"}</span>{" "}
              <code>{decision.name}</code>
            </span>
            <Badge tone={decision.allow ? "ok" : "danger"}>{decision.allow ? "Allowed" : "Denied"}</Badge>
            <span className="ee-att-acts">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void remember(decision.pairing, decision.kind, decision.name, !decision.allow)}
              >
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
          kind={adding}
          onCancel={() => setAdding(null)}
          onDecide={async (pairing, name, allow) => {
            if (await remember(pairing, adding, name, allow)) setAdding(null);
          }}
        />
      )}
      <AllowedHosts hosts={access.allowedHosts} onChange={setHosts} />
      <div className="ee-agent-note">
        Agents use this Entry with <code>keefast-agent run</code>, or ask keefast to send it in an HTTP request, only to the
        hosts above. Shells and interpreters, such as bash or python, are never remembered: they ask every time.
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

/** The hosts `http_request` may send the Entry's secrets to. A host that isn't one stays in the
 * field, with the reason in a toast. */
function AllowedHosts({ hosts, onChange }: { hosts: string[]; onChange: (hosts: string[]) => Promise<boolean> }) {
  const [host, setHost] = useState("");
  const add = async () => {
    if (!host.trim()) return;
    if (await onChange([...hosts, host.trim()])) setHost("");
  };
  return (
    <div className="ee-agent-hosts">
      <div className="ee-agent-hosts-head">Allowed hosts</div>
      {hosts.length === 0 ? (
        <div className="ee-empty">No host yet: HTTP requests can't send this Entry's secrets anywhere.</div>
      ) : (
        <div className="ee-agent-chips">
          {hosts.map((allowed) => (
            <span key={allowed} className="ee-agent-chip">
              <code>{allowed}</code>
              <button
                type="button"
                aria-label={`Remove ${allowed}`}
                onClick={() => void onChange(hosts.filter((other) => other !== allowed))}
              >
                <X />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="ee-agent-add">
        <Input
          aria-label="Allowed host"
          placeholder="api.github.com or *.example.com"
          value={host}
          onChange={(event) => setHost(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void add();
          }}
        />
        <Button size="sm" variant="ghost" disabled={!host.trim()} onClick={() => void add()}>
          Add
        </Button>
      </div>
    </div>
  );
}

function AddDecision({
  access,
  kind,
  onCancel,
  onDecide,
}: {
  access: EntryAgentAccess;
  kind: AgentUseKind;
  onCancel: () => void;
  onDecide: (pairing: string, name: string, allow: boolean) => Promise<void>;
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
      <span className="subtle">{kind === "host" ? "at" : "with"}</span>
      <Input
        aria-label={kind === "host" ? "Host" : "Program"}
        placeholder={kind === "host" ? "api.github.com" : "gh"}
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
