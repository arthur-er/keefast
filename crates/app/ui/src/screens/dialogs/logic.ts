// What the request dialogs decide without React: which request is asked now, and moving through
// a captured login's places.

import type { GroupId, GroupNode, LoginOption, LoginTarget, WorkspaceSnapshot } from "@/bindings";

type Pending<K extends keyof WorkspaceSnapshot> = NonNullable<WorkspaceSnapshot[K]>;

/** What waits for the user's answer, one at a time. */
export type FrontRequest =
  | { kind: "lock"; lock: Pending<"pendingLock"> }
  | { kind: "pairing"; pairing: Pending<"pendingPairing"> }
  | { kind: "access"; access: Pending<"pendingAccess"> }
  | { kind: "login"; login: Pending<"pendingLogin"> };

/** The request asked now: a Vault locking with unsaved changes, else a Pairing request over an
 * Access request over a captured login, as the GPUI app (and the backend's `browserRequest`)
 * order them. The others wait their turn. */
export function frontRequest(snapshot: WorkspaceSnapshot | undefined): FrontRequest | null {
  if (!snapshot) return null;
  if (snapshot.pendingLock) return { kind: "lock", lock: snapshot.pendingLock };
  if (snapshot.pendingPairing) return { kind: "pairing", pairing: snapshot.pendingPairing };
  if (snapshot.pendingAccess) return { kind: "access", access: snapshot.pendingAccess };
  if (snapshot.pendingLogin) return { kind: "login", login: snapshot.pendingLogin };
  return null;
}

/** Tells requests apart, so a new one gets a fresh dialog. */
export function requestKey(request: FrontRequest): string {
  switch (request.kind) {
    case "lock":
      return `lock-${request.lock.vault}`;
    case "pairing":
      return `pairing-${request.pairing.id}`;
    case "access":
      return `access-${request.access.id}`;
    case "login":
      return `login-${request.login.id}`;
  }
}

export function sameTarget(a: LoginTarget, b: LoginTarget): boolean {
  if (a.kind === "update" && b.kind === "update") return a.vault === b.vault && a.entry === b.entry;
  if (a.kind === "newEntry" && b.kind === "newEntry") return a.vault === b.vault && a.group === b.group;
  return false;
}

/** The place `step` rows from `chosen` (headings skipped), stopping at the ends. */
export function stepTarget(options: readonly LoginOption[], chosen: LoginTarget, step: number): LoginTarget {
  const targets = options.flatMap((option) => (option.target ? [option.target] : []));
  if (targets.length === 0) return chosen;
  const at = targets.findIndex((target) => sameTarget(target, chosen));
  const next = at < 0 ? 0 : Math.max(0, Math.min(targets.length - 1, at + step));
  return targets[next]!;
}

/** A Group option's label, split from the "(new Group)" the backend adds to the Browser group it
 * would make. */
export function splitNewGroup(label: string): { name: string; isNew: boolean } {
  const suffix = " (new Group)";
  return label.endsWith(suffix) ? { name: label.slice(0, -suffix.length), isNew: true } : { name: label, isNew: false };
}

/** The Group in `tree` and what is in it, or `null`. */
export function findGroup(tree: GroupNode, id: GroupId): GroupNode | null {
  if (tree.id === id) return tree;
  for (const child of tree.children) {
    const found = findGroup(child, id);
    if (found) return found;
  }
  return null;
}

/** The Groups below `group`, at any depth. */
export function groupsInside(group: GroupNode): GroupNode[] {
  return group.children.flatMap((child) => [child, ...groupsInside(child)]);
}

/** "6 Entries and 1 Group inside", or "Empty". */
export function contentsLine(group: GroupNode): string {
  const entries = group.totalEntries;
  const groups = groupsInside(group).length;
  const parts = [];
  if (entries > 0) parts.push(`${entries} ${entries === 1 ? "Entry" : "Entries"}`);
  if (groups > 0) parts.push(`${groups} ${groups === 1 ? "Group" : "Groups"}`);
  return parts.length === 0 ? "Empty" : `${parts.join(" and ")} inside`;
}

/** A site's address split into its scheme, shown muted, and the rest: "https://" and
 * "example.com:8443". */
export function splitOrigin(origin: string): { scheme: string; rest: string } {
  const at = origin.indexOf("://");
  return at < 0 ? { scheme: "", rest: origin } : { scheme: origin.slice(0, at + 3), rest: origin.slice(at + 3) };
}
