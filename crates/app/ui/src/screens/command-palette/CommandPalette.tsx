// Command palette A: classic centred, grouped sections (DESIGN.md §7). Ctrl K opens it (Ctrl
// Shift P too). With nothing typed it lists every command the screens on display registered
// (`useAppCommand`), grouped by section, with their keys; typing ranks commands and Entries
// together with keefast's fuzzy matching. Enter on an Entry opens a page of its actions (copy,
// open, go to it, edit, history…); Ctrl C / B / T on a highlighted Entry copy at once.
//
// A screen's commands show here by registering them with `useAppCommand`; nothing here needs
// to change. Sections the palette doesn't know yet are listed by name before "App".

import { type CSSProperties, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Command } from "cmdk";
import { Dialog as DialogPrimitive } from "radix-ui";
import {
  ArrowDownAZ, ArrowRight, ArrowUpDown, CalendarPlus, Check, ChevronRight, Clock, CornerDownLeft, ExternalLink, History,
  KeyRound, Link, type LucideIcon, Pencil, RefreshCw, SearchX, Settings2, TextCursorInput, Timer, UserRound, Vault, X,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { EntryDetail, EntrySummary, Scope, SortOrder } from "@/bindings";
import { useEntry, useTotpCode, useWorkspace } from "@/api/queries";
import { Kbd, toast } from "@/kit";
import { highlightRuns } from "@/lib/fuzzy";
import { hostOf } from "@/lib/format";
import { openHistory } from "@/screens/entry-history/open";
import { copyField, openUrl } from "@/screens/vault-browser/actions";
import { EntryAvatar } from "@/screens/vault-browser/EntryList";
import { type AppCommand, useAppCommands } from "@/shell/commands";
import { useShell } from "@/shell/state";
import { showView, useView } from "@/shell/view";

import { isConfident, orderSections, rankByTitle, rankEntries } from "./rank";

import "./palette.css";

/** An action the palette lists: a registered command, one of the palette's own, or one of an
 * Entry's actions. */
interface Action {
  kind: "action";
  /** Unique in the list. */
  value: string;
  title: string;
  section: string;
  icon?: LucideIcon;
  shortcut?: string;
  enabled: boolean;
  /** On the right, before the keys: a value ("arthur-er"), a TOTP code, a count. */
  trail?: ReactNode;
  /** Opens a page of the palette rather than running and closing. */
  page?: boolean;
  /** A ticked option, such as the sort order in use. */
  checked?: boolean;
  /** Matched characters of the title. */
  positions?: number[];
  score?: number;
  run: () => void;
}

interface EntryItem {
  kind: "entry";
  value: string;
  entry: EntrySummary;
  positions: number[];
  score: number;
}

type Item = Action | EntryItem;

interface Section {
  key: string;
  heading: ReactNode;
  items: Item[];
}

type Page = { kind: "entry"; entry: EntrySummary } | { kind: "sort" } | { kind: "fields"; entry: EntrySummary };

const SORTS: { order: SortOrder; name: string; phrase: string; icon: LucideIcon }[] = [
  { order: "title", name: "Title", phrase: "title", icon: ArrowDownAZ },
  { order: "username", name: "Username", phrase: "username", icon: UserRound },
  { order: "url", name: "URL", phrase: "URL", icon: Link },
  { order: "created", name: "Creation time", phrase: "creation time", icon: CalendarPlus },
  { order: "modified", name: "Last change", phrase: "last change", icon: Clock },
];

const MASK = "••••••••••";

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const { state, actions } = useShell();
  const view = useView();
  const snapshot = useWorkspace().data;
  const selectedKey = view?.entry ?? null;
  const selected = useEntry(selectedKey?.vault, selectedKey?.entry).data;
  const registered = useAppCommands();
  const [query, setQuery] = useState("");
  const [page, setPage] = useState<Page | null>(null);
  const [highlighted, setHighlighted] = useState("");
  const input = useRef<HTMLInputElement>(null);

  // Every Entry of the unlocked Vaults, for Entries to come up as the user types.
  const entries = useQuery({
    queryKey: ["entries", "palette"],
    queryFn: () => api.listEntries({ scope: { kind: "allVaults" }, search: "", sort: "title" }),
    staleTime: 0,
  }).data;

  const pageEntry = page && page.kind !== "sort" ? page.entry : null;
  const pageDetail = useEntry(pageEntry?.vault, pageEntry?.id).data;

  /** Selects an Entry in the list (keeping the scope when it shows the Entry) and its detail. */
  const goTo = (entry: EntrySummary) => {
    const scope: Scope = view && scopeShows(view.scope, entry) ? view.scope : { kind: "vault", vault: entry.vault };
    showView({ scope, entry: { vault: entry.vault, entry: entry.id } });
    actions.showBrowser();
  };

  const entryActions = (entry: EntrySummary, detail: EntryDetail | undefined, section: string): Action[] => {
    const id = `${entry.vault}:${entry.id}`;
    const list: Action[] = [
      { kind: "action", value: `act:${id}:password`, section, title: "Copy password", icon: KeyRound, shortcut: "Ctrl C", enabled: entry.hasPassword, trail: <span className="pal-mono">{MASK}</span>, run: () => void copyField(entry.vault, entry.id, { kind: "password" }, "Password") },
      { kind: "action", value: `act:${id}:username`, section, title: "Copy username", icon: UserRound, shortcut: "Ctrl B", enabled: entry.username !== "", trail: entry.username && <span className="ctx">{entry.username}</span>, run: () => void copyField(entry.vault, entry.id, { kind: "username" }, "Username") },
    ];
    if (entry.hasTotp) {
      list.push({ kind: "action", value: `act:${id}:totp`, section, title: "Copy TOTP code", icon: Timer, shortcut: "Ctrl T", enabled: true, trail: <TotpTrail entry={entry} />, run: () => void copyField(entry.vault, entry.id, { kind: "totp" }, "TOTP code") });
    }
    detail?.customFields.forEach((field, index) =>
      list.push({ kind: "action", value: `act:${id}:field:${index}`, section, title: `Copy ${field.name}`, icon: TextCursorInput, enabled: !field.isEmpty, trail: <span className="ctx">Custom field</span>, run: () => void copyField(entry.vault, entry.id, { kind: "customField", index }, field.name) }),
    );
    if (entry.url) {
      list.push({ kind: "action", value: `act:${id}:url`, section, title: `Open ${hostOf(entry.url)}`, icon: ExternalLink, enabled: true, run: () => openUrl(entry.vault, entry.id) });
    }
    list.push(
      { kind: "action", value: `act:${id}:goto`, section, title: "Show in the Entry list", icon: ArrowRight, enabled: true, run: () => goTo(entry) },
      { kind: "action", value: `act:${id}:edit`, section, title: "Edit Entry", icon: Pencil, shortcut: "Enter", enabled: true, run: () => (goTo(entry), actions.editEntry()) },
      {
        kind: "action", value: `act:${id}:history`, section, title: "Show Entry history", icon: History, shortcut: "Ctrl H", enabled: true,
        trail: detail && detail.historyCount > 0 && <span className="ctx">{detail.historyCount} versions</span>,
        run: () => (goTo(entry), openHistory(actions)),
      },
      { kind: "action", value: `act:${id}:vault`, section, title: "Vault settings", icon: Settings2, enabled: true, trail: <span className="ctx">{entry.vaultName}</span>, run: () => actions.openVaultSettings(entry.vault) },
    );
    return list;
  };

  // The palette's own commands: pages for the sort order and the custom fields, and syncing.
  const builtIns = (searching: boolean): Action[] => {
    const list: Action[] = [];
    if (selected && selected.customFields.length > 0) {
      list.push({ kind: "action", value: "own:fields", section: "Entry", title: "Copy custom field…", icon: TextCursorInput, enabled: true, page: true, run: () => openPage({ kind: "fields", entry: selected }) });
    }
    if (snapshot?.vaults.some((vault) => vault.syncStatus !== null)) {
      list.push({ kind: "action", value: "own:sync", section: "Vaults", title: "Sync Dropbox Vaults now", icon: RefreshCw, enabled: true, run: () => void api.syncNow().catch((error) => toast(errorMessage(error), { icon: "error" })) });
    }
    if (searching) list.push(...SORTS.map((sort) => sortAction(sort, "View")));
    else {
      const current = SORTS.find((sort) => sort.order === state.sort);
      list.push({ kind: "action", value: "own:sort", section: "View", title: "Sort Entries by…", icon: ArrowUpDown, enabled: true, page: true, trail: <span className="ctx">{current?.name}</span>, run: () => openPage({ kind: "sort" }) });
    }
    return list;
  };

  const sortAction = (sort: (typeof SORTS)[number], section: string, short = false): Action => ({
    kind: "action", value: `own:sort:${sort.order}`, section, title: short ? sort.name : `Sort Entries by ${sort.phrase}`, icon: sort.icon, enabled: true,
    checked: state.sort === sort.order, run: () => actions.setSort(sort.order),
  });

  const searching = query.trim() !== "";
  const commandActions: Action[] = registered
    .filter((command) => !command.hidden)
    .map((command) => fromCommand(command, searching && selected && command.section === "Entry" ? selected.title : undefined));

  const sections = buildSections();

  function buildSections(): Section[] {
    if (page?.kind === "sort") {
      const sorts = SORTS.map((sort) => sortAction(sort, "", true));
      return [{ key: "sort", heading: null, items: searching ? rankActions(query, sorts) : sorts }];
    }
    if (page) {
      const all = entryActions(page.entry, pageDetail, "");
      const shown = page.kind === "fields" ? all.filter((action) => action.value.includes(":field:")) : all;
      return [{ key: "page", heading: null, items: searching ? rankActions(query, shown) : shown }];
    }
    const own = builtIns(searching);
    if (!searching) {
      const all = [...commandActions, ...own];
      return orderSections(all.map((action) => action.section)).map((section) => ({
        key: section,
        heading: section === "Entry" && selected ? <EntryHeading entry={selected} /> : section,
        items: inOrder(all.filter((action) => action.section === section)),
      }));
    }
    const ranked = rankActions(query, [...commandActions, ...own]);
    const found = rankEntries(query, (entries ?? []).filter((entry) => !entry.inRecycleBin)).slice(0, 5);
    const entrySection: Section | null = found.length
      ? { key: "entries", heading: "Entries", items: found.map(({ item, score, positions }) => ({ kind: "entry", value: `entry:${item.vault}:${item.id}`, entry: item, positions, score })) }
      : null;
    const top = found[0];
    const actionsFor: Section | null =
      top && isConfident(found)
        ? {
            key: "actionsFor",
            heading: <EntryHeading entry={top.item} prefix="Actions for " />,
            items: entryActions(top.item, undefined, "").filter((action) => /:(password|username|totp|url)$/.test(action.value)),
          }
        : null;
    const commandSection: Section | null = ranked.length ? { key: "commands", heading: "Commands", items: ranked } : null;
    const commandsFirst = commandSection && entrySection && (ranked[0]?.score ?? 0) > found[0]!.score;
    const ordered = commandsFirst ? [commandSection, entrySection, actionsFor] : [entrySection, actionsFor, commandSection];
    return ordered.filter((section): section is Section => section !== null);
  }

  const items = sections.flatMap((section) => section.items);
  const firstEnabled = items.find((item) => item.kind === "entry" || item.enabled)?.value ?? "";
  const highlightedItem = items.find((item) => item.value === highlighted);
  // A new query or page highlights its first item.
  const shown = `${page?.kind ?? ""}:${query}`;
  useEffect(() => setHighlighted(firstEnabled), [shown]);
  useEffect(() => {
    if (!highlightedItem && firstEnabled) setHighlighted(firstEnabled);
  }, [highlightedItem, firstEnabled]);

  function openPage(next: Page | null) {
    setPage(next);
    setQuery("");
    input.current?.focus();
  }

  const run = (item: Item) => {
    if (item.kind === "entry") return openPage({ kind: "entry", entry: item.entry });
    if (!item.enabled) return;
    if (item.page) return item.run();
    onClose();
    item.run();
  };

  const runOnEntry = (entry: EntrySummary, what: "password" | "username" | "totp") => {
    onClose();
    if (what === "password") void copyField(entry.vault, entry.id, { kind: "password" }, "Password");
    else if (what === "username") void copyField(entry.vault, entry.id, { kind: "username" }, "Username");
    else void copyField(entry.vault, entry.id, { kind: "totp" }, "TOTP code");
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Tab") {
      event.preventDefault();
      return;
    }
    if (event.key === "Backspace" && query === "" && page) {
      event.preventDefault();
      openPage(null);
      return;
    }
    // Ctrl C / B / T copy from the highlighted Entry, or the one whose actions show, unless
    // text is selected in the query.
    const target = highlightedItem?.kind === "entry" ? highlightedItem.entry : page?.kind === "entry" ? page.entry : null;
    if ((event.ctrlKey || event.metaKey) && target && input.current?.selectionStart === input.current?.selectionEnd) {
      const what = { c: "password", b: "username", t: "totp" } as const;
      const field = what[event.key.toLowerCase() as keyof typeof what];
      if (field && (field !== "totp" || target.hasTotp)) {
        event.preventDefault();
        runOnEntry(target, field);
      }
    }
  };

  const placeholder = page?.kind === "entry" ? `Search actions for ${page.entry.title}…` : page?.kind === "fields" ? "Pick a custom field…" : page ? "Pick a sort order…" : "Type a command or search Entries…";

  return (
    <DialogPrimitive.Root open onOpenChange={(open) => !open && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="scrim pal-scrim">
          <DialogPrimitive.Content
            className="pal"
            aria-describedby={undefined}
            onEscapeKeyDown={(event) => {
              if (page) {
                event.preventDefault();
                openPage(null);
              }
            }}
            onOpenAutoFocus={(event) => {
              event.preventDefault();
              input.current?.focus();
            }}
          >
            <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
            <Command shouldFilter={false} loop vimBindings={false} value={highlighted} onValueChange={setHighlighted} onKeyDown={onKeyDown} label="Command palette">
              <div className="pal-ctx">
                <ContextChips page={page} selected={selected} onBack={() => openPage(null)} scopeName={scopeName(view?.scope, snapshot?.vaults)} />
              </div>
              <div className="pal-q">
                <Command.Input ref={input} value={query} onValueChange={setQuery} placeholder={placeholder} autoComplete="off" spellCheck={false} />
              </div>
              <Command.List className="pal-list" label="Commands and Entries">
                {items.length === 0 && (
                  <div className="pal-empty">
                    <SearchX />
                    <div>{page ? "No matching actions" : "No matching commands"}</div>
                    {query.trim() && <div className="subtle">Nothing matches “{query.trim()}”{page ? "." : " in commands or Entries."}</div>}
                  </div>
                )}
                {sections.map((section) =>
                  section.heading === null ? (
                    <Command.Group key={section.key}>{section.items.map((item) => renderItem(item))}</Command.Group>
                  ) : (
                    <Command.Group key={section.key} heading={section.heading}>
                      {section.items.map((item) => renderItem(item))}
                    </Command.Group>
                  ),
                )}
              </Command.List>
              <Footer item={highlightedItem} inPage={page !== null} />
            </Command>
          </DialogPrimitive.Content>
        </DialogPrimitive.Overlay>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );

  function renderItem(item: Item) {
    if (item.kind === "entry") {
      const { entry } = item;
      const isHighlighted = item.value === highlighted;
      const place = entry.groupName && entry.groupName !== entry.vaultName ? `${entry.vaultName} / ${entry.groupName}` : entry.vaultName;
      return (
        <Command.Item key={item.value} value={item.value} className="pi" onSelect={() => run(item)}>
          <EntryAvatar entry={entry} size={20} />
          <span className="lbl fit">
            <Marked text={entry.title} positions={item.positions} />
          </span>
          <span className="sub">{entry.username}</span>
          <span className="trail">
            {isHighlighted ? (
              <>
                {entry.hasPassword && <QuickAction label="Copy password (Ctrl C)" icon={KeyRound} onClick={() => runOnEntry(entry, "password")} />}
                {entry.hasTotp && <QuickAction label="Copy TOTP code (Ctrl T)" icon={Timer} onClick={() => runOnEntry(entry, "totp")} />}
                <QuickAction label="Actions (Enter)" icon={CornerDownLeft} onClick={() => openPage({ kind: "entry", entry })} />
              </>
            ) : (
              <span className="ctx">{place}</span>
            )}
          </span>
        </Command.Item>
      );
    }
    const Icon = item.icon;
    return (
      <Command.Item key={item.value} value={item.value} className="pi" disabled={!item.enabled} onSelect={() => run(item)}>
        {Icon && <Icon />}
        <span className="lbl">
          <Marked text={item.title} positions={item.positions ?? []} />
        </span>
        <span className="trail">
          {item.trail}
          {item.checked && <Check className="tick" />}
          {item.shortcut && <Kbd combo={item.shortcut} />}
          {item.page && <ChevronRight className="chev" />}
        </span>
      </Command.Item>
    );
  }
}

function fromCommand(command: AppCommand, entryTitle: string | undefined): Action {
  return {
    kind: "action",
    value: `cmd:${command.id}`,
    title: command.title,
    section: command.section,
    icon: command.icon,
    shortcut: command.shortcut?.[0],
    enabled: command.enabled ?? true,
    trail: entryTitle !== undefined ? <EntryContext title={entryTitle} /> : undefined,
    run: command.run,
  };
}

/** The selected Entry's name beside its commands while searching (listed by section, the
 * heading says it). */
function EntryContext({ title }: { title: string }) {
  return <span className="ctx">{title}</span>;
}

/** Commands the palette puts first in their section, as the prototype lists them, and last. */
const FIRST = ["cmd:copyPassword", "cmd:copyUsername", "cmd:copyTotp", "own:fields", "cmd:editEntry", "cmd:showHistory"];
const LAST = ["cmd:deleteEntry"];

/** A section's actions: the known ones in the prototype's order, the others as registered. */
function inOrder(actions: Action[]): Action[] {
  const rank = (action: Action) => {
    const first = FIRST.indexOf(action.value);
    if (first >= 0) return first - FIRST.length;
    return LAST.includes(action.value) ? 1 : 0;
  };
  return actions.map((action, index) => ({ action, index })).sort((a, b) => rank(a.action) - rank(b.action) || a.index - b.index).map(({ action }) => action);
}

function rankActions(query: string, actions: readonly Action[]): Action[] {
  return rankByTitle(query, actions).map(({ item, score, positions }) => ({ ...item, score, positions }));
}

/** Whether the list showing `scope` shows the Entry. */
function scopeShows(scope: Scope, entry: EntrySummary): boolean {
  switch (scope.kind) {
    case "allVaults":
      return true;
    case "vault":
      return scope.vault === entry.vault;
    case "group":
      return scope.vault === entry.vault && scope.group === entry.group;
    case "tag":
      return entry.tags.includes(scope.tag);
    case "color":
      return entry.color === scope.color;
  }
}

function scopeName(scope: Scope | undefined, vaults: readonly { id: number; name: string }[] | undefined): string | null {
  if (!scope) return null;
  switch (scope.kind) {
    case "allVaults":
      return "All Entries";
    case "tag":
      return scope.tag;
    case "color":
      return scope.color[0]!.toUpperCase() + scope.color.slice(1);
    default:
      return vaults?.find((vault) => vault.id === scope.vault)?.name ?? null;
  }
}

function ContextChips({ page, selected, scopeName, onBack }: { page: Page | null; selected: EntrySummary | undefined; scopeName: string | null; onBack: () => void }) {
  const back = (
    <button type="button" className="x" aria-label="Back" onClick={onBack}>
      <X />
    </button>
  );
  if (page?.kind === "sort") {
    return (
      <span className="pal-chip plain is-page">
        <ArrowUpDown />
        <span>Sort Entries by…</span>
        {back}
      </span>
    );
  }
  if (page) {
    return (
      <span className="pal-chip is-page">
        <EntryAvatar entry={page.entry} size={16} />
        <span>{page.kind === "fields" ? `Custom fields of ${page.entry.title}` : page.entry.title}</span>
        {back}
      </span>
    );
  }
  const vault = selected?.vaultName ?? scopeName;
  return (
    <>
      {vault && (
        <span className="pal-chip plain">
          <Vault />
          <span>{vault}</span>
        </span>
      )}
      {selected && (
        <>
          <span className="pal-chip-sep">
            <ChevronRight />
          </span>
          <span className="pal-chip">
            <EntryAvatar entry={selected} size={16} />
            <span>{selected.title}</span>
          </span>
        </>
      )}
    </>
  );
}

function EntryHeading({ entry, prefix = "" }: { entry: EntrySummary; prefix?: string }) {
  return (
    <>
      <EntryAvatar entry={entry} size={16} />
      <span className="truncate">
        {prefix}
        {entry.title}
      </span>
    </>
  );
}

function Marked({ text, positions }: { text: string; positions: readonly number[] }) {
  if (positions.length === 0) return <>{text}</>;
  return (
    <>
      {highlightRuns(text, positions).map((run, index) => (run.matched ? <mark key={index}>{run.text}</mark> : <span key={index}>{run.text}</span>))}
    </>
  );
}

function QuickAction({ label, icon: Icon, onClick }: { label: string; icon: LucideIcon; onClick: () => void }) {
  return (
    <button
      type="button"
      className="pal-qa"
      title={label}
      aria-label={label}
      // The query keeps the focus.
      onMouseDown={(event) => event.preventDefault()}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <Icon />
    </button>
  );
}

/** The Entry's TOTP code as it runs, beside Copy TOTP code. */
function TotpTrail({ entry }: { entry: EntrySummary }) {
  const totp = useTotpCode(entry.vault, entry.id).data;
  const [now, setNow] = useState(() => Date.now());
  // When the code came, which its seconds left count from.
  const fetchedAt = useMemo(() => (totp ? Date.now() : 0), [totp]);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  if (!totp) return null;
  const left = Math.max(0, totp.remaining - Math.floor((now - fetchedAt) / 1000));
  const half = Math.ceil(totp.code.length / 2);
  return (
    <span className="pal-totp">
      <span className="ring" style={{ "--p": left / totp.period } as CSSProperties} />
      {totp.code.slice(0, half)} {totp.code.slice(half)}
    </span>
  );
}

function Footer({ item, inPage }: { item: Item | undefined; inPage: boolean }) {
  const hint = (keys: ReactNode, label: string) => (
    <span className="h">
      {keys}
      <span>{label}</span>
    </span>
  );
  const keys = (...names: string[]) => (
    <span className="kbds">
      {names.map((name) => (
        <kbd key={name}>{name}</kbd>
      ))}
    </span>
  );
  return (
    <div className="pal-foot">
      {item?.kind === "entry" ? (
        <>
          {hint(keys("↵"), "Actions")}
          {item.entry.hasPassword && hint(keys("Ctrl", "C"), "Copy password")}
          {item.entry.hasTotp && hint(keys("Ctrl", "T"), "Copy TOTP")}
        </>
      ) : item ? (
        <>
          {hint(keys("↑", "↓"), "Navigate")}
          {hint(keys("↵"), item.page ? "Open" : "Run")}
        </>
      ) : (
        hint(keys("⌫"), "Edit query")
      )}
      <span className="spacer" />
      {hint(keys("Esc"), inPage ? "Back" : "Close")}
    </div>
  );
}
