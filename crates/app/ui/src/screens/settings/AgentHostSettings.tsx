// Settings → Agent hosts (ADR 0012 "Approvals"), laid out as Settings C: the keefast Headless
// hosts this desktop is an Approver for, each with how its connection stands, and adding one by
// its address and the token `keefast-agent pairing approver` printed on it. Their prompts show
// as the approval prompt, over everything (screens/dialogs/RequestDialogs.tsx). Each host's
// Activity, its audit log read through the Approver connection, opens from its row.

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Info, Plus, RefreshCw, Server } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { AgentActivityRecord, AgentHostView } from "@/bindings";
import { useAgentHosts } from "@/api/queries";
import { Badge, Button, Dialog, Field, Input, Popover, PopoverContent, PopoverTrigger, Segmented, Select, toast } from "@/kit";

import { type ActivityFilter, describeActivity, filterActivity, NO_FILTER, type OutcomeFilter, pairingsIn } from "./activity";

import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection } from "./SettingsLayout";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

const HOW = "A Headless host asks before an Agent uses a secret in a way nobody has allowed yet. Its prompts show here while keefast runs; the first answer from any of your desktops counts. Unanswered, a request is denied after a minute.";
const PAIR = "On the host, run keefast-agent pairing approver <name> and paste the token it prints.";

export function AgentHostSettings({ section }: { section?: string }) {
  const hosts = useAgentHosts().data;
  const [adding, setAdding] = useState(false);

  const sections = useMemo<SettingsSection[]>(() => {
    if (!hosts) return [];
    return [
      {
        id: "hosts",
        name: "Hosts",
        desc: "This desktop answers these hosts' prompts.",
        count: hosts.length,
        cards: [
          [
            ...hosts.map((host) => ({
              id: host.id,
              search: `${host.name} ${host.url} ${host.state} ${host.problem ?? ""}`,
              render: () => <HostRow host={host} />,
            })),
            {
              id: "add",
              search: `Add an Agent host address token Approver pair ${PAIR}`,
              render: () => (
                <SettingRow label="Add an Agent host" desc={<Hl>{PAIR}</Hl>}>
                  <Button size="sm" onClick={() => setAdding(true)}>
                    <Plus /> Add host…
                  </Button>
                </SettingRow>
              ),
            },
            {
              id: "how",
              search: `prompts Approver minute denied ${HOW}`,
              render: () => (
                <div className="set-note">
                  <Info />
                  <span>
                    <Hl>{HOW}</Hl>
                  </span>
                </div>
              ),
            },
          ],
        ],
      },
    ];
  }, [hosts]);

  return (
    <>
      <SettingsLayout
        crumbs={
          <>
            Settings <CrumbSep /> <b>Agent hosts</b>
          </>
        }
        searchLabel="Search Agent hosts"
        title="Agent hosts"
        sub={<>keefast Headless hosts serve a Vault to AI Agents. Pair this desktop with one as an Approver to answer its prompts. Hosts are kept on this machine, not in a Vault.</>}
        sections={sections}
        tip="Changes apply immediately."
        emptyLabel="Agent hosts"
        section={section}
      />
      {adding && <AddHostDialog onClose={() => setAdding(false)} />}
    </>
  );
}

const STATE_LABEL: Record<AgentHostView["state"], string> = {
  connecting: "Connecting",
  connected: "Connected",
  disconnected: "Not connected",
};

function HostRow({ host }: { host: AgentHostView }) {
  const [removing, setRemoving] = useState(false);
  const [watching, setWatching] = useState(false);
  const remove = () => {
    setRemoving(false);
    api.removeAgentHost(host.id).then(() => toast(<>Removed <b>{host.name}</b></>, { icon: "check" }), fail);
  };
  return (
    <div className="set-row pair">
      <div className="pair-ico">
        <Server className="ico-sm" />
      </div>
      <div className="txt">
        <div className="lbl hstack gap-2">
          <span className="truncate">
            <Hl>{host.name}</Hl>
          </span>
          <Badge tone={host.state === "connected" ? "ok" : host.state === "disconnected" ? "warn" : "neutral"}>{STATE_LABEL[host.state]}</Badge>
        </div>
        <div className="desc pair-meta truncate" title={host.url}>
          <Hl>{host.url}</Hl>
        </div>
        {host.problem && (
          <div className="desc" role="status">
            {host.problem}
          </div>
        )}
      </div>
      <div className="ctl hstack gap-2">
        <Button size="sm" onClick={() => setWatching(true)}>
          Activity…
        </Button>
        {watching && <ActivityDialog host={host} onClose={() => setWatching(false)} />}
        <Popover open={removing} onOpenChange={setRemoving}>
          <PopoverTrigger asChild>
            <Button size="sm">Remove</Button>
          </PopoverTrigger>
          <PopoverContent
            side="top"
            align="end"
            className="confirm-pop"
            onEscapeKeyDown={(event) => {
              event.preventDefault();
              setRemoving(false);
            }}
          >
            <h4>Stop answering “{host.name}”?</h4>
            <p>Its prompts stop showing here. To shut this desktop out on the host too, run keefast-agent pairing revoke-approver there.</p>
            <div className="hstack justify-end">
              <Button size="sm" variant="ghost" onClick={() => setRemoving(false)}>
                Cancel
              </Button>
              <Button size="sm" variant="danger" onClick={remove}>
                Remove
              </Button>
            </div>
          </PopoverContent>
        </Popover>
      </div>
    </div>
  );
}

export function AddHostDialog({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ready = url.trim() !== "" && token.trim() !== "";

  const add = async () => {
    setBusy(true);
    setError(null);
    try {
      const host = await api.addAgentHost(name, url, token);
      toast(
        <>
          Added <b>{host.name}</b>
        </>,
        { icon: "check" },
      );
      onClose();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(next) => next || onClose()}
      title="Add an Agent host"
      description="This desktop becomes an Approver for it, and shows its prompts while keefast runs."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || !ready} onClick={() => void add()}>
            Add
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && ready) void add();
        }}
      >
        <Field label="Address" htmlFor="agent-host-url" hint="The address Agents use, such as https://keefast.example.net:7807.">
          <Input id="agent-host-url" autoFocus mono value={url} placeholder="https://" onChange={(event) => setUrl(event.target.value)} />
        </Field>
        <Field label="Approver token" htmlFor="agent-host-token" error={error} hint={PAIR}>
          <Input
            id="agent-host-token"
            mono
            type="password"
            autoComplete="off"
            value={token}
            placeholder="kfapprover_…"
            invalid={error !== null}
            onChange={(event) => setToken(event.target.value)}
          />
        </Field>
        <Field label="Name" htmlFor="agent-host-name" hint="Optional: its address names it otherwise.">
          <Input id="agent-host-name" value={name} placeholder="Build server" onChange={(event) => setName(event.target.value)} />
        </Field>
      </form>
    </Dialog>
  );
}

const OUTCOMES = [
  { value: "all", label: "All" },
  { value: "allowed", label: "Allowed" },
  { value: "denied", label: "Denied" },
] as const satisfies readonly { value: OutcomeFilter; label: string }[];

const ALL_PAIRINGS = "*";

/** A Headless host's Activity: what its Agents did and were refused, newest first. */
export function ActivityDialog({ host, onClose }: { host: AgentHostView; onClose: () => void }) {
  const activity = useQuery({ queryKey: ["agentActivity", host.id], queryFn: () => api.getAgentActivity(host.id), retry: false });
  const [filter, setFilter] = useState<ActivityFilter>(NO_FILTER);
  const records = activity.data ?? [];
  const shown = filterActivity(records, filter);
  const pairings = pairingsIn(records);

  return (
    <Dialog
      open
      onOpenChange={(next) => next || onClose()}
      className="activity-dialog"
      title={<>Activity on {host.name}</>}
      description="What Agents asked this host for, and whether they got it. Values are never recorded."
      footer={
        <>
          <Button variant="ghost" disabled={activity.isFetching} onClick={() => void activity.refetch()}>
            <RefreshCw /> Refresh
          </Button>
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </>
      }
    >
      <div className="activity-filters">
        <Select
          label="Agent pairing"
          value={filter.pairing || ALL_PAIRINGS}
          onValueChange={(pairing) => setFilter({ ...filter, pairing: pairing === ALL_PAIRINGS ? "" : pairing })}
          options={[{ value: ALL_PAIRINGS, label: "All pairings" }, ...pairings.map((pairing) => ({ value: pairing, label: pairing }))]}
          className="activity-pairing"
        />
        <Input aria-label="Entry" placeholder="Entry" value={filter.entry} onChange={(event) => setFilter({ ...filter, entry: event.target.value })} />
        <Segmented label="Outcome" value={filter.outcome} onValueChange={(outcome) => setFilter({ ...filter, outcome })} options={OUTCOMES} />
      </div>
      {activity.isError ? (
        <div className="activity-empty" role="alert">
          {errorMessage(activity.error)}
        </div>
      ) : activity.isPending ? (
        <div className="activity-empty">Reading the host’s log…</div>
      ) : shown.length === 0 ? (
        <div className="activity-empty">{records.length === 0 ? "No Agent has asked this host for anything yet." : "Nothing matches these filters."}</div>
      ) : (
        <ol className="activity-list" aria-label="Activity">
          {shown.map((record, at) => (
            <ActivityRow key={`${record.time}-${at}`} record={record} />
          ))}
        </ol>
      )}
    </Dialog>
  );
}

function ActivityRow({ record }: { record: AgentActivityRecord }) {
  const when = record.time === null ? "" : new Date(record.time * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  return (
    <li className="activity-row">
      <div className="activity-what">
        <span className="truncate">{describeActivity(record)}</span>
        <Badge tone={record.outcome === "allowed" ? "ok" : "danger"}>{record.outcome === "allowed" ? "Allowed" : "Denied"}</Badge>
      </div>
      <div className="activity-meta">
        <span>{record.pairing}</span>
        <span>{when}</span>
        {record.reason && <span>{record.reason}</span>}
      </div>
    </li>
  );
}
