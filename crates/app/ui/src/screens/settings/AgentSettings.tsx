// Vault settings → Agents (ADR 0012): the Agent group, whose subgroups are all Agents ever see,
// and the Agent pairings: each owns a subgroup by its name, may read others, and may create
// Entries in its own. Pairing an Agent shows its bearer token once; the Vault keeps only its hash.

import { useState } from "react";
import { Bot, Copy, History, Plus } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { AgentPairingView, AgentSettingsView, GroupId, VaultId } from "@/bindings";
import { Button, Check, Dialog, Field, Input, Popover, PopoverTrigger, toast } from "@/kit";

import { ActivityDialog } from "./AgentHostSettings";
import { ConfirmPop, RowPop } from "./ConfirmPop";
import { GroupPicker } from "./GroupPicker";
import { Hl, SettingRow, type SettingsSection } from "./SettingsLayout";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

const SECTION_NOTE = "AI agents reach this Vault through keefast's MCP server. They see only Entries in the Agent group's subgroups, and never their notes.";
const GROUP_NOTE = "Each Agent gets a subgroup of it. Entries anywhere else never reach an Agent.";
const CREATE_LABEL = "Creates Entries in its own subgroup";
const CREATE_NOTE = "It can save secrets it mints, and set their fields. It never deletes, and its changes stay in each Entry's history.";

/** The names of the subgroups a pairing reads, its own first. */
export function readsText(pairing: AgentPairingView, settings: AgentSettingsView): string {
  const name = (id: GroupId) => settings.subgroups.find((group) => group.id === id)?.name;
  const own = name(pairing.group) ?? pairing.name;
  const others = pairing.reads.map(name).filter((n): n is string => n !== undefined);
  const reads = others.length > 0 ? `Reads ${own}, ${others.join(", ")}` : `Reads ${own}`;
  return pairing.canCreate ? `${reads} · creates Entries` : reads;
}

export function agentsSection(vault: VaultId, settings: AgentSettingsView, vaultName: string): SettingsSection {
  return {
    id: "agents",
    name: "Agents",
    count: settings.pairings.length,
    desc: SECTION_NOTE,
    cards: [
      [
        {
          id: "agentGroup",
          search: `Agent group ${GROUP_NOTE} agents mcp ai subgroup`,
          render: () => (
            <SettingRow label="Agent group" desc={GROUP_NOTE}>
              <GroupPicker
                vault={vault}
                label="Agent group"
                value={settings.agentGroup}
                valueName={settings.agentGroupName}
                placeholder="Not chosen"
                menuLabel="Agents reach Entries here"
                binNote="The Agent group can't be in the Recycle Bin."
                onPick={(group) => void api.setAgentGroup(vault, group).catch(fail)}
              />
            </SettingRow>
          ),
        },
        {
          id: "pairAgent",
          search: "Pair an Agent token bearer mcp claude codex new pairing",
          render: () => <PairAgentRow vault={vault} settings={settings} />,
        },
        {
          id: "agentActivity",
          search: "Activity audit log Agents on this computer keefast-agent mcp list use denied",
          render: () => <ActivityRow vault={vault} vaultName={vaultName} />,
        },
      ],
      settings.pairings.length > 0
        ? settings.pairings.map((pairing) => ({
            id: `agent-${pairing.id}`,
            search: `${pairing.name} Agent pairing reads subgroups creates entries revoke`,
            render: () => <AgentPairingRow vault={vault} settings={settings} pairing={pairing} />,
          }))
        : [{
            id: "noAgents",
            search: "No Agent is paired with this Vault",
            render: () => (
              <div className="set-row">
                <div className="txt">
                  <div className="desc mt-0 flex items-center gap-2">
                    <Bot className="ico-sm" /> No Agent is paired with this Vault.
                  </div>
                </div>
              </div>
            ),
          }],
    ],
  };
}

function PairAgentRow({ vault, settings }: { vault: VaultId; settings: AgentSettingsView }) {
  const [open, setOpen] = useState(false);
  const ready = settings.agentGroup !== null;
  return (
    <SettingRow
      label="Pair an Agent"
      desc={ready ? "Makes the Agent its own subgroup and a token to connect with." : "Choose the Agent group first."}
    >
      <Button size="sm" disabled={!ready} onClick={() => setOpen(true)}>
        <Plus /> <Hl>Pair an Agent…</Hl>
      </Button>
      {open && <PairAgentDialog vault={vault} settings={settings} onClose={() => setOpen(false)} />}
    </SettingRow>
  );
}

/** This Vault's Activity in the in-app host: what Agents on this computer did with it. */
function ActivityRow({ vault, vaultName }: { vault: VaultId; vaultName: string }) {
  const [open, setOpen] = useState(false);
  return (
    <SettingRow label="Activity" desc="What Agents on this computer asked this Vault for, through keefast-agent, and whether they got it.">
      <Button size="sm" onClick={() => setOpen(true)}>
        <History /> <Hl>Activity…</Hl>
      </Button>
      {open && <ActivityDialog vault={{ id: vault, name: vaultName }} onClose={() => setOpen(false)} />}
    </SettingRow>
  );
}

/** The other subgroups a pairing may read, as checkboxes. */
function ReadsChecklist({
  settings,
  except,
  reads,
  onChange,
}: {
  settings: AgentSettingsView;
  except: GroupId | null;
  reads: GroupId[];
  onChange: (reads: GroupId[]) => void;
}) {
  const others = settings.subgroups.filter((group) => group.id !== except);
  if (others.length === 0) return <div className="hint">The Agent group has no other subgroups to read.</div>;
  return (
    <div className="flex flex-col gap-2">
      {others.map((group) => (
        <Check
          key={group.id}
          checked={reads.includes(group.id)}
          onCheckedChange={(on) => onChange(on ? [...reads, group.id] : reads.filter((id) => id !== group.id))}
        >
          <span className="truncate">{group.name}</span>
        </Check>
      ))}
    </div>
  );
}

/** Names the Agent and picks what else it reads, then shows its token, once. */
export function PairAgentDialog({ vault, settings, onClose }: { vault: VaultId; settings: AgentSettingsView; onClose: () => void }) {
  const [name, setName] = useState("");
  const [reads, setReads] = useState<GroupId[]>([]);
  const [canCreate, setCanCreate] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const own = settings.subgroups.find((group) => group.name === name.trim())?.id ?? null;

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const created = await api.createAgentPairing(vault, name, reads.filter((id) => id !== own), canCreate);
      setToken(created.token);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  if (token !== null) {
    const copy = () =>
      navigator.clipboard.writeText(token).then(
        () => toast("Token copied", { icon: "check" }),
        () => toast("The token couldn't be copied: select it and copy it.", { icon: "error" }),
      );
    return (
      <Dialog
        open
        onOpenChange={(next) => next || onClose()}
        title={`“${name.trim()}” is paired`}
        description="Give the Agent this token. keefast shows it only now: the Vault keeps only its hash."
        footer={
          <Button variant="primary" autoFocus onClick={onClose}>
            Done
          </Button>
        }
      >
        <div className="flex flex-col gap-3">
          <div className="hstack gap-2">
            <Input mono readOnly aria-label="Token" value={token} className="flex-1" onFocus={(event) => event.currentTarget.select()} />
            <Button onClick={() => void copy()}>
              <Copy /> Copy
            </Button>
          </div>
          <div className="hint">
            The Agent connects to keefast's MCP server with the header <code>Authorization: Bearer &lt;token&gt;</code>.
          </div>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog
      open
      onOpenChange={(next) => next || onClose()}
      title="Pair an Agent"
      description={`It gets its own subgroup of ${settings.agentGroupName ?? "the Agent group"}, by its name, and reads the Entries there.`}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || name.trim() === ""} onClick={() => void create()}>
            Pair
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && name.trim() !== "") void create();
        }}
      >
        <Field label="Name" htmlFor="agent-name" error={error}>
          <Input id="agent-name" autoFocus value={name} placeholder="Claude Code on my laptop" invalid={error !== null} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Field label="Also reads">
          <ReadsChecklist settings={settings} except={own} reads={reads} onChange={setReads} />
        </Field>
        <CreateCheck checked={canCreate} onChange={setCanCreate} />
      </form>
    </Dialog>
  );
}

/** Whether a pairing creates Entries in its own subgroup, with what that lets it do. */
function CreateCheck({ checked, onChange }: { checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <div className="flex flex-col gap-1">
      <Check checked={checked} onCheckedChange={onChange}>
        {CREATE_LABEL}
      </Check>
      <div className="hint">{CREATE_NOTE}</div>
    </div>
  );
}

/** A paired Agent: what it reads and whether it creates Entries, which the user can change, and
 * Revoke once confirmed. */
function AgentPairingRow({ vault, settings, pairing }: { vault: VaultId; settings: AgentSettingsView; pairing: AgentPairingView }) {
  const [reading, setReading] = useState(false);
  const [revoking, setRevoking] = useState(false);
  const revoke = () => {
    setRevoking(false);
    api.revokeAgentPairing(vault, pairing.id).then(() => toast(`Agent “${pairing.name}” revoked`, { icon: "check" }), fail);
  };
  return (
    <div className="set-row pair">
      <div className="pair-ico">
        <Bot className="ico-sm" />
      </div>
      <div className="txt">
        <div className="lbl truncate">
          <Hl>{pairing.name}</Hl>
        </div>
        <div className="desc pair-meta truncate">{readsText(pairing, settings)}</div>
      </div>
      <div className="ctl">
        <Popover open={reading} onOpenChange={setReading}>
          <PopoverTrigger asChild>
            <Button size="sm" variant="ghost">
              Access…
            </Button>
          </PopoverTrigger>
          <RowPop onClose={() => setReading(false)}>
            <h4>What “{pairing.name}” also reads</h4>
            <p>Besides its own subgroup.</p>
            <ReadsChecklist
              settings={settings}
              except={pairing.group}
              reads={pairing.reads}
              onChange={(reads) => void api.setAgentPairingAccess(vault, pairing.id, reads, pairing.canCreate).catch(fail)}
            />
            <CreateCheck
              checked={pairing.canCreate}
              onChange={(on) => void api.setAgentPairingAccess(vault, pairing.id, pairing.reads, on).catch(fail)}
            />
          </RowPop>
        </Popover>
        <Popover open={revoking} onOpenChange={setRevoking}>
          <PopoverTrigger asChild>
            <Button size="sm">Revoke</Button>
          </PopoverTrigger>
          <ConfirmPop title={`Revoke the Agent “${pairing.name}”?`} action="Revoke" onCancel={() => setRevoking(false)} onConfirm={revoke}>
            Its token stops working at once. Its subgroup and Entries stay.
          </ConfirmPop>
        </Popover>
      </div>
    </div>
  );
}

