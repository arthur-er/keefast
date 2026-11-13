// The Groups command palette A's "Move to Group…" page offers an Entry (palette C's move page).

import type { GroupId, GroupNode } from "@/bindings";

export interface MoveTarget {
  group: GroupNode;
  /** 0 for the Vault's top Group. */
  depth: number;
  /** Whether the Entry is in it already. */
  current: boolean;
}

/** The Groups an Entry in `current` can move to, in tree order with their depth: every Group of
 * its Vault but the Recycle Bin and what is in it (deleting is how an Entry gets there). */
export function moveTargets(root: GroupNode, current: GroupId): MoveTarget[] {
  const out: MoveTarget[] = [];
  const walk = (group: GroupNode, depth: number) => {
    if (group.inRecycleBin) return;
    out.push({ group, depth, current: group.id === current });
    group.children.forEach((child) => walk(child, depth + 1));
  };
  walk(root, 0);
  return out;
}

/** "3 Entries", "1 Entry", "No Entries": what is right in a Group. */
export function entriesIn(group: GroupNode): string {
  const count = group.entryCount;
  return count === 0 ? "No Entries" : count === 1 ? "1 Entry" : `${count} Entries`;
}
