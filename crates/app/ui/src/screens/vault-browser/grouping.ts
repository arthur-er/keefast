// Vault browser C's Entry list: Entries under a heading per Group, Groups collapsible, and the
// keyboard moving through the Entries shown.

import type { EntryKey, EntrySummary, GroupId, Scope, VaultId } from "@/bindings";

/** An Entry's key in the list, unique across Vaults. */
export const entryKey = (entry: { vault: VaultId; id: string }) => `${entry.vault}:${entry.id}`;

export const sameEntry = (a: EntryKey | null | undefined, b: EntrySummary) =>
  a != null && a.vault === b.vault && a.entry === b.id;

export interface EntryGroup {
  /** `<vault>:<group>`: what collapsing remembers. */
  key: string;
  label: string;
  /** Whether it is a Vault's top Group, which heads the list. */
  top: boolean;
  entries: EntrySummary[];
}

export interface GroupingOptions {
  /** Each open Vault's top Group, to head a Vault's list with its own Entries. */
  topGroups: ReadonlyMap<VaultId, GroupId>;
  /** Whether the list spans Vaults (all Vaults, a tag, a colour): headings then say the Vault. */
  acrossVaults: boolean;
  /** Whether a search ranks the list: Groups then come in the order of their best match. */
  searching: boolean;
}

/** The Entries under a heading per Group, in the order the backend listed them within each.
 * Unsearched, a Vault's top Group comes first, then the others by name; searched, Groups come in
 * the order of their best match, so the best Entry stays first. */
export function groupEntries(entries: readonly EntrySummary[], options: GroupingOptions): EntryGroup[] {
  const groups = new Map<string, EntryGroup & { vault: VaultId; first: number }>();
  entries.forEach((entry, index) => {
    const key = `${entry.vault}:${entry.group}`;
    let group = groups.get(key);
    if (!group) {
      const top = options.topGroups.get(entry.vault) === entry.group;
      const name = top ? entry.vaultName : entry.groupName;
      const label = options.acrossVaults && !top ? `${entry.vaultName} / ${name}` : name;
      group = { key, label, top, entries: [], vault: entry.vault, first: index };
      groups.set(key, group);
    }
    group.entries.push(entry);
  });
  const sorted = [...groups.values()].sort((a, b) => {
    if (options.searching) return a.first - b.first;
    if (a.vault !== b.vault) return a.vault - b.vault;
    if (a.top !== b.top) return a.top ? -1 : 1;
    return a.label.localeCompare(b.label, undefined, { sensitivity: "base" });
  });
  return sorted.map(({ key, label, top, entries: list }) => ({ key, label, top, entries: list }));
}

/** The Entries the list shows, top to bottom: those of collapsed Groups are hidden. */
export function visibleEntries(groups: readonly EntryGroup[], collapsed: ReadonlySet<string>): EntrySummary[] {
  return groups.flatMap((group) => (collapsed.has(group.key) ? [] : group.entries));
}

export type Move = "next" | "previous" | "first" | "last";

/** The Entry to select after a keyboard move from `current` (none selected moves to the first or
 * last). `null` when the list is empty. */
export function moveSelection(
  shown: readonly EntrySummary[],
  current: EntryKey | null | undefined,
  move: Move,
): EntrySummary | null {
  if (shown.length === 0) return null;
  const at = shown.findIndex((entry) => sameEntry(current, entry));
  const last = shown.length - 1;
  switch (move) {
    case "first":
      return shown[0]!;
    case "last":
      return shown[last]!;
    case "next":
      return shown[at < 0 ? 0 : Math.min(at + 1, last)]!;
    case "previous":
      return shown[at < 0 ? last : Math.max(at - 1, 0)]!;
  }
}

/** Whether the list leaves Recycle Bin Entries out: a Vault's (or every Vault's) Entries are
 * those outside it, as the sidebar counts them; the Recycle Bin is a Group of its own. Tags and
 * colours leave it out already. */
export function hidesRecycleBin(scope: Scope): boolean {
  return scope.kind === "vault" || scope.kind === "allVaults";
}

/** Whether the list spans Vaults. */
export function spansVaults(scope: Scope): boolean {
  return scope.kind === "allVaults" || scope.kind === "tag" || scope.kind === "color";
}
