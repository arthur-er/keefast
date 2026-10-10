// Vault browser C's Entry list: search, sort, and the Entries under a heading per Group, the
// keyboard moving through them.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown, ArrowUp, ArrowUpDown, ChevronDown, Copy, ExternalLink, FolderPlus, KeyRound, Paperclip, Plus, Search,
  SearchX, SquarePen, Timer, Trash2, User,
} from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EntrySummary, GroupId, SortOrder, VaultId } from "@/bindings";
import { useEntries, useWorkspace } from "@/api/queries";
import {
  Badge, Button, colorVar, ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger,
  EntryIcon, Kbd, Menu, MenuContent, MenuItem, MenuLabel, MenuTrigger, toast, Tooltip,
} from "@/kit";
import { highlight } from "@/lib/format";
import { SHORTCUTS } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { startNewEntry } from "@/screens/entry-editor/session";
import { useShell } from "@/shell/state";
import { showView, useView } from "@/shell/view";

import { copyField, deleteEntry, openUrl } from "./actions";
import {
  entryKey, groupEntries, hidesRecycleBin, moveSelection, type Move, sameEntry, spansVaults, visibleEntries,
} from "./grouping";

const SORTS: { value: SortOrder; label: string }[] = [
  { value: "title", label: "Title" },
  { value: "username", label: "Username" },
  { value: "url", label: "URL" },
  { value: "modified", label: "Modified" },
  { value: "created", label: "Created" },
];

export function EntryList() {
  const { state, actions } = useShell();
  const view = useView();
  const snapshot = useWorkspace().data;
  const keysActive = useKeysActive();
  const searchField = useRef<HTMLInputElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const scope = view?.scope ?? { kind: "allVaults" as const };
  const query = useEntries({ scope, search: state.search, sort: state.sort });
  const searching = state.search.trim() !== "";

  const entries = useMemo(
    () => (query.data ?? []).filter((entry) => !(hidesRecycleBin(scope) && entry.inRecycleBin)),
    [query.data, scope],
  );
  const topGroups = useMemo(
    () => new Map((snapshot?.vaults ?? []).flatMap((vault) => (vault.rootGroup ? [[vault.id, vault.rootGroup] as const] : []))),
    [snapshot?.vaults],
  );
  const groups = useMemo(
    () => groupEntries(entries, { topGroups, acrossVaults: spansVaults(scope), searching }),
    [entries, topGroups, scope, searching],
  );
  const shown = useMemo(() => visibleEntries(groups, collapsed), [groups, collapsed]);
  const selected = view?.entry ?? null;

  // A list with nothing selected (or the selection gone) selects its first Entry.
  useEffect(() => {
    if (!view || query.isPlaceholderData || !query.data) return;
    if (selected && entries.some((entry) => sameEntry(selected, entry))) return;
    const first = shown[0];
    if (first) showView({ scope: view.scope, entry: { vault: first.vault, entry: first.id } });
    else if (selected) showView({ scope: view.scope, entry: null });
  }, [view, selected, entries, shown, query.isPlaceholderData, query.data]);

  // The selected row stays in sight as the keyboard moves.
  useEffect(() => {
    if (!selected) return;
    body.current
      ?.querySelector(`[data-entry="${CSS.escape(`${selected.vault}:${selected.entry}`)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const select = (entry: EntrySummary) => {
    // Entry editor A stays until it has asked about unsaved changes.
    if (state.detail !== "edit") actions.showEntry();
    if (view) showView({ scope: view.scope, entry: { vault: entry.vault, entry: entry.id } });
  };
  const move = (how: Move) => {
    const next = moveSelection(shown, selected, how);
    if (next) select(next);
  };

  useAppCommand({ id: "search", title: "Search Entries", section: "Go to", icon: Search, shortcut: SHORTCUTS.search, run: () => searchField.current?.focus() }, keysActive);
  useAppCommand({ id: "nextEntry", title: "Next Entry", section: "Go to", hidden: true, shortcut: SHORTCUTS.nextEntry, run: () => move("next") }, keysActive);
  useAppCommand({ id: "previousEntry", title: "Previous Entry", section: "Go to", hidden: true, shortcut: SHORTCUTS.previousEntry, run: () => move("previous") }, keysActive);
  useAppCommand({ id: "firstEntry", title: "First Entry", section: "Go to", hidden: true, shortcut: SHORTCUTS.firstEntry, run: () => move("first") }, keysActive);
  useAppCommand({ id: "lastEntry", title: "Last Entry", section: "Go to", hidden: true, shortcut: SHORTCUTS.lastEntry, run: () => move("last") }, keysActive);
  useAppCommand(
    {
      id: "deleteEntry",
      title: "Delete Entry",
      section: "Entry",
      icon: Trash2,
      shortcut: SHORTCUTS.deleteEntry,
      enabled: selected !== null,
      run: () => selected && void deleteEntry(selected.vault, selected.entry, actions),
    },
    keysActive,
  );

  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <section className="vb-list" aria-label="Entries">
      <div className="vb-list-head">
        <label className="search">
          <Search className="ico-sm" />
          <input
            ref={searchField}
            value={state.search}
            placeholder="Search Entries"
            aria-label="Search Entries"
            onChange={(event) => actions.setSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                if (state.search) actions.setSearch("");
                else event.currentTarget.blur();
              }
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                move(event.key === "ArrowDown" ? "next" : "previous");
              }
            }}
          />
          {!state.search && <Kbd combo="/" />}
        </label>
        <SortMenu />
        <AddMenu />
      </div>
      <div ref={body} className="vb-list-body" role="list">
        {groups.length === 0 ? (
          query.data && <EmptyList searching={searching} onClear={() => actions.setSearch("")} />
        ) : (
          groups.map((group) => {
            const isCollapsed = collapsed.has(group.key);
            return (
              <div key={group.key} role="group" aria-label={group.label}>
                <button
                  type="button"
                  className={cn("C-ghead", isCollapsed && "is-collapsed")}
                  aria-expanded={!isCollapsed}
                  onClick={() => toggle(group.key)}
                >
                  <ChevronDown className="chev" />
                  <span className="truncate">{group.label}</span>
                  <span className="count">{group.entries.length}</span>
                </button>
                {!isCollapsed &&
                  group.entries.map((entry) => (
                    <EntryRow
                      key={entryKey(entry)}
                      entry={entry}
                      selected={sameEntry(selected, entry)}
                      search={state.search}
                      onSelect={() => select(entry)}
                    />
                  ))}
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}

function EntryRow({
  entry,
  selected,
  search,
  onSelect,
}: {
  entry: EntrySummary;
  selected: boolean;
  search: string;
  onSelect: () => void;
}) {
  const { actions } = useShell();
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          role="listitem"
          aria-current={selected ? "true" : undefined}
          data-entry={entryKey(entry)}
          className={cn("C-row", selected && "is-selected")}
          onMouseDown={onSelect}
          onContextMenu={onSelect}
        >
          <EntryAvatar entry={entry} size={20} />
          <span className="t truncate">
            <Marked text={entry.title} query={search} />
          </span>
          <span className="u truncate">
            <Marked text={entry.username} query={search} />
          </span>
          <span className="m">
            {entry.expired && <Badge tone="danger" className="vb-badge-exp">Expired</Badge>}
            {entry.attachmentCount > 0 && <Paperclip aria-label="Has attachments" />}
            {entry.hasTotp && <Timer aria-label="Has TOTP" />}
            {entry.color && <span className="dot" style={{ background: colorVar(entry.color) }} />}
          </span>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem icon={<User />} shortcut="Ctrl B" disabled={!entry.username} onSelect={() => void copyField(entry.vault, entry.id, { kind: "username" }, "Username")}>
          Copy username
        </ContextMenuItem>
        <ContextMenuItem icon={<KeyRound />} shortcut="Ctrl C" disabled={!entry.hasPassword} onSelect={() => void copyField(entry.vault, entry.id, { kind: "password" }, "Password")}>
          Copy password
        </ContextMenuItem>
        <ContextMenuItem icon={<Timer />} shortcut="Ctrl T" disabled={!entry.hasTotp} onSelect={() => void copyField(entry.vault, entry.id, { kind: "totp" }, "TOTP code")}>
          Copy TOTP code
        </ContextMenuItem>
        <ContextMenuItem icon={<Copy />} disabled={!entry.url} onSelect={() => void copyField(entry.vault, entry.id, { kind: "url" }, "URL")}>
          Copy URL
        </ContextMenuItem>
        <ContextMenuItem icon={<ExternalLink />} disabled={!entry.url} onSelect={() => openUrl(entry.vault, entry.id)}>
          Open URL
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem icon={<Trash2 />} shortcut="Delete" danger onSelect={() => void deleteEntry(entry.vault, entry.id, actions)}>
          {entry.inRecycleBin ? "Delete for good…" : "Delete"}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** An Entry's picture on a tile: its favicon, custom icon or standard icon. */
export function EntryAvatar({ entry, size }: { entry: Pick<EntrySummary, "icon" | "faviconUrl">; size: number }) {
  const image = entry.faviconUrl !== null || entry.icon.kind === "custom";
  const radius = size >= 40 ? 12 : size >= 24 ? 7 : 5;
  return (
    <span className={cn("vb-avatar", image && "is-image")} style={{ width: size, height: size, borderRadius: radius }}>
      <EntryIcon icon={entry.icon} faviconUrl={entry.faviconUrl} size={image ? size : Math.round(size * 0.6)} />
    </span>
  );
}

function Marked({ text, query }: { text: string; query: string }) {
  const parts = highlight(text, query);
  if (!parts) return <>{text}</>;
  return (
    <>
      {parts.before}
      <mark className="vb-hl">{parts.match}</mark>
      {parts.after}
    </>
  );
}

function EmptyList({ searching, onClear }: { searching: boolean; onClear: () => void }) {
  if (searching) {
    return (
      <div className="vb-empty">
        <SearchX />
        <p>No Entries match the search</p>
        <Button size="sm" onClick={onClear} shortcut="Esc">
          Clear search
        </Button>
      </div>
    );
  }
  return (
    <div className="vb-empty">
      <KeyRound />
      <p>No Entries</p>
    </div>
  );
}

function SortMenu() {
  const { state, actions } = useShell();
  return (
    <Menu>
      <Tooltip content="Sort">
        <MenuTrigger asChild>
          <button type="button" className="btn ghost sm icon" aria-label="Sort Entries">
            <ArrowUpDown />
          </button>
        </MenuTrigger>
      </Tooltip>
      <MenuContent align="end">
        <MenuLabel>Sort by</MenuLabel>
        {SORTS.map((sort) => (
          <MenuItem
            key={sort.value}
            icon={sort.value === "modified" || sort.value === "created" ? <ArrowDown /> : <ArrowUp />}
            onSelect={() => actions.setSort(sort.value)}
          >
            {sort.label}
            {state.sort === sort.value && <span className="sub">✓</span>}
          </MenuItem>
        ))}
      </MenuContent>
    </Menu>
  );
}

function AddMenu() {
  const { actions } = useShell();
  const view = useView();
  const newGroup = async () => {
    if (!view) return;
    try {
      const target: { vault: VaultId; group: GroupId } | null = await api.groupForNewItems(view.scope);
      if (!target) return void toast("Pick a Vault first.", { icon: "info" });
      const group = await api.createGroup(target.vault, target.group, "New Group");
      actions.renameGroup(`${target.vault}:${group}`);
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  return (
    <Menu>
      <Tooltip content="New">
        <MenuTrigger asChild>
          <button type="button" className="btn ghost sm icon" aria-label="New">
            <Plus />
          </button>
        </MenuTrigger>
      </Tooltip>
      <MenuContent align="end">
        <MenuItem icon={<SquarePen />} shortcut="Ctrl N" onSelect={() => void startNewEntry(view, actions)}>
          New Entry
        </MenuItem>
        <MenuItem icon={<FolderPlus />} onSelect={() => void newGroup()}>
          New Group
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}
