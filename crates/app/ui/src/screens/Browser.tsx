// Placeholder: a plain Vault list, Entry list and Entry detail proving the round trip (views,
// queries, copy, reveal, TOTP, events). PR 2 replaces it with the Vault browser C shell.

import { useEffect, useRef, useState } from "react";
import { Copy, Eye, EyeOff, FolderPlus, Layers, Lock, LockOpen, LogOut, Plus, Save, Search, Settings } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EntryDetail, Scope, SortOrder, VaultSummary, View, WorkspaceSnapshot } from "@/api";
import { useEntries, useEntry, useTotpCode } from "@/api/queries";
import { Badge, Button, ColorDot, EntryIcon, IconButton, Kbd, Select, Tag, toast, Tooltip } from "@/kit";
import { SHORTCUTS, useShortcut } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

const SORTS: { value: SortOrder; label: string }[] = [
  { value: "title", label: "Title" },
  { value: "username", label: "Username" },
  { value: "url", label: "URL" },
  { value: "created", label: "Created" },
  { value: "modified", label: "Modified" },
];

function sameScope(a: Scope, b: Scope) {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function BrowserScreen({
  snapshot,
  onOpenAnother,
}: {
  snapshot: WorkspaceSnapshot;
  onOpenAnother: () => void;
}) {
  const view = snapshot.view;
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortOrder>("title");
  const entries = useEntries({ scope: view.scope, search, sort });
  const searchField = useRef<HTMLInputElement>(null);
  const show = (next: View) => void api.setView(next);

  useShortcut(SHORTCUTS.search, () => searchField.current?.focus());
  useShortcut(["ArrowDown", "ArrowUp"], (event) => {
    const list = entries.data ?? [];
    if (list.length === 0) return;
    const at = list.findIndex((e) => e.vault === view.entry?.vault && e.id === view.entry.entry);
    const next = event.key === "ArrowDown" ? Math.min(at + 1, list.length - 1) : Math.max(at - 1, 0);
    const entry = list[next]!;
    show({ scope: view.scope, entry: { vault: entry.vault, entry: entry.id } });
  });

  const selectScope = (scope: Scope) => show({ scope, entry: null });

  return (
    <div className="flex h-full">
      <aside className="flex w-[var(--sidebar-w)] flex-none flex-col border-r border-border bg-bg-sidebar">
        <nav className="nav-section flex-1 overflow-auto">
          <div
            className={cn("nav-item", view.scope.kind === "allVaults" && "is-selected")}
            onClick={() => selectScope({ kind: "allVaults" })}
          >
            <Layers /> All Vaults
          </div>
          <div className="nav-head">Vaults</div>
          {snapshot.vaults.map((vault) => (
            <VaultRow
              key={vault.id}
              vault={vault}
              selected={view.scope.kind === "vault" && view.scope.vault === vault.id}
              onSelect={() => selectScope({ kind: "vault", vault: vault.id })}
              onUnlock={onOpenAnother}
            />
          ))}
          {snapshot.tags.length > 0 && <div className="nav-head">Tags</div>}
          {snapshot.tags.map((tag) => (
            <div
              key={tag}
              className={cn("nav-item", sameScope(view.scope, { kind: "tag", tag }) && "is-selected")}
              onClick={() => selectScope({ kind: "tag", tag })}
            >
              <span className="dot bg-fg-2" /> {tag}
            </div>
          ))}
        </nav>
        <div className="hstack border-t border-border p-2">
          <Button size="sm" variant="ghost" onClick={onOpenAnother}>
            <Plus /> Open / New
          </Button>
          <span className="spacer" />
          <Tooltip content="Settings" shortcut="Ctrl ,">
            <IconButton size="sm" label="Settings" onClick={() => toast("Settings arrive with the settings screen.", { icon: "info" })}>
              <Settings />
            </IconButton>
          </Tooltip>
          <Tooltip content="Lock all Vaults" shortcut="Ctrl L">
            <IconButton size="sm" label="Lock all Vaults" onClick={() => void api.lockAll()}>
              <LogOut />
            </IconButton>
          </Tooltip>
        </div>
      </aside>

      <section className="flex w-[var(--list-w)] flex-none flex-col border-r border-border">
        <div className="toolbar">
          <label className="search flex-1">
            <Search className="ico-sm" />
            <input
              ref={searchField}
              value={search}
              placeholder="Search"
              onChange={(event) => setSearch(event.target.value)}
              onKeyDown={(event) => event.key === "Escape" && setSearch("")}
            />
            <Kbd combo="Ctrl F" />
          </label>
          <Select value={sort} onValueChange={setSort} options={SORTS} label="Sort Entries by" className="w-28" />
        </div>
        <div className="flex-1 overflow-auto p-1.5">
          {(entries.data ?? []).map((entry) => {
            const selected = view.entry?.vault === entry.vault && view.entry.entry === entry.id;
            return (
              <div
                key={`${entry.vault}:${entry.id}`}
                className={cn("row h-11", selected && "is-selected")}
                onClick={() => show({ scope: view.scope, entry: { vault: entry.vault, entry: entry.id } })}
              >
                <EntryIcon icon={entry.icon} faviconUrl={entry.faviconUrl} size={22} />
                <div className="min-w-0 flex-1">
                  <div className="truncate">{entry.title}</div>
                  <div className="truncate text-sm text-fg-2">{entry.username}</div>
                </div>
                {entry.color && <ColorDot color={entry.color} />}
                {entry.expired && <Badge tone="danger">Expired</Badge>}
              </div>
            );
          })}
          {entries.data?.length === 0 && (
            <div className="p-6 text-center text-fg-2">{search ? "No Entries match the search" : "No Entries"}</div>
          )}
        </div>
        <StatusBar snapshot={snapshot} />
      </section>

      <main className="min-w-0 flex-1 overflow-auto">
        {view.entry ? (
          <EntryPane vault={view.entry.vault} entry={view.entry.entry} />
        ) : (
          <div className="grid h-full place-items-center text-fg-2">Select an Entry</div>
        )}
      </main>
    </div>
  );
}

function VaultRow({
  vault,
  selected,
  onSelect,
  onUnlock,
}: {
  vault: VaultSummary;
  selected: boolean;
  onSelect: () => void;
  onUnlock: () => void;
}) {
  const locked = vault.state !== "unlocked";
  return (
    <div className={cn("nav-item", selected && "is-selected")} onClick={vault.state === "locked" ? onUnlock : onSelect}>
      {locked ? <Lock /> : <LockOpen />}
      <span className="truncate">
        {vault.name}
        {vault.unsavedChanges && " •"}
        {vault.state === "locking" && " (locking…)"}
      </span>
      {!locked && <span className="count">{vault.entryCount}</span>}
      {vault.syncStatus && <span className="count">{vault.syncStatus}</span>}
    </div>
  );
}

function StatusBar({ snapshot }: { snapshot: WorkspaceSnapshot }) {
  const words = { saving: "Saving…", failed: "Save failed", unsaved: "Unsaved changes", saved: "All changes saved" };
  const tone = { saving: "var(--fg-2)", failed: "var(--danger)", unsaved: "var(--warn)", saved: "var(--ok)" };
  return (
    <div className="hstack h-8 border-t border-border px-3 text-sm text-fg-1">
      <span className="status-dot" style={{ background: tone[snapshot.saveStatus] }} />
      {words[snapshot.saveStatus]}
      <span className="spacer" />
      <Tooltip content="Save all Vaults" shortcut="Ctrl S">
        <IconButton size="sm" label="Save all Vaults" onClick={() => void api.saveAll()}>
          <Save />
        </IconButton>
      </Tooltip>
      <Tooltip content="New Group">
        <IconButton
          size="sm"
          label="New Group"
          onClick={async () => {
            const target = await api.groupForNewItems(snapshot.view.scope);
            if (!target) return toast("Pick a Vault first.", { icon: "info" });
            await api.createGroup(target.vault, target.group, "New Group");
          }}
        >
          <FolderPlus />
        </IconButton>
      </Tooltip>
    </div>
  );
}

function EntryPane({ vault, entry }: { vault: number; entry: string }) {
  const detail = useEntry(vault, entry);
  if (detail.error) return <div className="p-6 text-danger">{errorMessage(detail.error)}</div>;
  if (!detail.data) return null;
  return <EntryDetailView key={`${vault}:${entry}`} entry={detail.data} />;
}

async function copy(entry: EntryDetail, field: Parameters<typeof api.copyField>[2], what: string) {
  try {
    const copied = await api.copyField(entry.vault, entry.id, field);
    toast(
      <>
        {what} copied{copied.clearsInSeconds !== null && <span className="subtle"> · clipboard clears in {copied.clearsInSeconds}s</span>}
      </>,
      { icon: "clipboard", seconds: copied.clearsInSeconds ?? undefined },
    );
  } catch (error) {
    toast(errorMessage(error), { icon: "info" });
  }
}

function EntryDetailView({ entry }: { entry: EntryDetail }) {
  const [password, setPassword] = useState<string | null>(null);
  const [fields, setFields] = useState<Record<number, string>>({});
  const totp = useTotpCode(entry.vault, entry.id, entry.totp?.status === "ready");

  useShortcut(SHORTCUTS.copyUsername, () => void copy(entry, { kind: "username" }, "Username"));
  useShortcut(SHORTCUTS.copyPassword, () => void copy(entry, { kind: "password" }, "Password"), { inFields: false });
  useShortcut(SHORTCUTS.copyTotp, () => void copy(entry, { kind: "totp" }, "TOTP code"));
  // A revealed secret goes when the Entry changes or the window is left.
  useEffect(() => {
    const hide = () => {
      setPassword(null);
      setFields({});
    };
    window.addEventListener("blur", hide);
    return () => window.removeEventListener("blur", hide);
  }, []);

  return (
    <div className="mx-auto flex max-w-[640px] flex-col gap-1 p-8">
      <div className="hstack mb-4 gap-3">
        <EntryIcon icon={entry.icon} faviconUrl={entry.faviconUrl} size={32} />
        <h1 className="m-0 truncate text-xl font-semibold tracking-tight">{entry.title}</h1>
        {entry.color && <ColorDot color={entry.color} />}
      </div>
      <div className="subtle mb-2 text-sm">{entry.groupPath.join(" / ")}</div>
      <Row label="Username" value={entry.username} onCopy={() => copy(entry, { kind: "username" }, "Username")} />
      <Row
        label="Password"
        mono
        value={password ?? (entry.hasPassword ? "••••••••••••" : "")}
        onCopy={() => copy(entry, { kind: "password" }, "Password")}
        reveal={{
          shown: password !== null,
          toggle: async () => setPassword(password === null ? await api.revealPassword(entry.vault, entry.id) : null),
        }}
      />
      {entry.totp?.status === "ready" && totp.data && (
        <Row
          label="TOTP"
          mono
          value={`${totp.data.code.slice(0, 3)} ${totp.data.code.slice(3)}  ·  ${totp.data.remaining}s`}
          onCopy={() => copy(entry, { kind: "totp" }, "TOTP code")}
        />
      )}
      {entry.totp?.status === "unreadable" && <Row label="TOTP" value={entry.totp.reason} />}
      <Row label="URL" value={entry.url} />
      {entry.notes && <Row label="Notes" value={entry.notes} />}
      {entry.customFields.map((field, index) => (
        <Row
          key={index}
          label={field.name}
          mono={field.protected}
          value={field.value ?? fields[index] ?? (field.isEmpty ? "" : "••••••••")}
          onCopy={() => copy(entry, { kind: "customField", index }, field.name)}
          reveal={
            field.protected
              ? {
                  shown: index in fields,
                  toggle: async () => {
                    const next = { ...fields };
                    if (index in next) delete next[index];
                    else next[index] = await api.revealCustomField(entry.vault, entry.id, index);
                    setFields(next);
                  },
                }
              : undefined
          }
        />
      ))}
      {entry.tags.length > 0 && (
        <div className="kv">
          <span className="k">Tags</span>
          <span className="v hstack flex-wrap gap-1">
            {entry.tags.map((tag) => (
              <Tag key={tag}>{tag}</Tag>
            ))}
          </span>
        </div>
      )}
      {entry.attachments.length > 0 && <Row label="Attachments" value={entry.attachments.map((a) => a.name).join(", ")} />}
      <div className="subtle mt-4 text-sm">
        {entry.historyCount} earlier version{entry.historyCount === 1 ? "" : "s"}
        {entry.modified !== null && ` · modified ${new Date(entry.modified * 1000).toLocaleDateString()}`}
      </div>
    </div>
  );
}

function Row({
  label,
  value,
  mono,
  onCopy,
  reveal,
}: {
  label: string;
  value: string;
  mono?: boolean;
  onCopy?: () => void;
  reveal?: { shown: boolean; toggle: () => void };
}) {
  return (
    <div className="kv group">
      <span className="k truncate">{label}</span>
      <span className="v hstack">
        <span className={cn("truncate selectable", mono && "mono")}>{value || <span className="subtle">—</span>}</span>
        <span className="spacer" />
        {reveal && (
          <IconButton size="sm" label={reveal.shown ? "Hide" : "Show"} onClick={reveal.toggle}>
            {reveal.shown ? <EyeOff /> : <Eye />}
          </IconButton>
        )}
        {onCopy && value && (
          <IconButton size="sm" label={`Copy ${label}`} className="opacity-0 group-hover:opacity-100" onClick={onCopy}>
            <Copy />
          </IconButton>
        )}
      </span>
    </div>
  );
}
