import { describe, expect, it } from "vitest";

import type { GroupNode, LoginOption, WorkspaceSnapshot } from "@/bindings";

import { contentsLine, findGroup, frontRequest, requestKey, sameTarget, splitNewGroup, stepTarget } from "./logic";

type Pending = Partial<Pick<WorkspaceSnapshot, "pendingLock" | "pendingPairing" | "pendingAccess" | "pendingLogin">>;
const none: Pending = { pendingLock: null, pendingPairing: null, pendingAccess: null, pendingLogin: null };
const snapshot = (pending: Pending) => ({ ...none, ...pending }) as unknown as WorkspaceSnapshot;

const lock = { vault: 2, vaultName: "Work", canCancel: true };
const pairing = { id: 7, vault: 0, vaultName: "Personal" };
const access = { id: 8, site: "github.com", entries: [] };
const login = {
  id: 9,
  site: "linear.app",
  username: "arthur",
  options: [],
  suggested: { kind: "newEntry" as const, vault: 0, group: null },
};

describe("frontRequest", () => {
  it("asks a lock first, then Pairing, Access and login requests in that order", () => {
    expect(frontRequest(snapshot({ pendingLock: lock, pendingPairing: pairing, pendingAccess: access, pendingLogin: login }))?.kind).toBe("lock");
    expect(frontRequest(snapshot({ pendingPairing: pairing, pendingAccess: access, pendingLogin: login }))?.kind).toBe("pairing");
    expect(frontRequest(snapshot({ pendingAccess: access, pendingLogin: login }))?.kind).toBe("access");
    expect(frontRequest(snapshot({ pendingLogin: login }))?.kind).toBe("login");
    expect(frontRequest(snapshot({}))).toBeNull();
    expect(frontRequest(undefined)).toBeNull();
  });

  it("tells requests apart by kind and number", () => {
    expect(requestKey(frontRequest(snapshot({ pendingAccess: access }))!)).toBe("access-8");
    expect(requestKey(frontRequest(snapshot({ pendingAccess: { ...access, id: 10 } }))!)).toBe("access-10");
    expect(requestKey(frontRequest(snapshot({ pendingLock: lock }))!)).toBe("lock-2");
  });
});

describe("a captured login's places", () => {
  const options: LoginOption[] = [
    { target: null, label: "Update an existing Entry", detail: null, icon: null, depth: 0 },
    { target: { kind: "update", vault: 1, entry: "w5" }, label: "Linear · arthur", detail: "Work", icon: null, depth: 1 },
    { target: null, label: "Save as a new Entry in", detail: null, icon: null, depth: 0 },
    { target: { kind: "newEntry", vault: 0, group: "root" }, label: "Personal", detail: null, icon: null, depth: 1 },
    { target: { kind: "newEntry", vault: 0, group: null }, label: "Browser (new Group)", detail: "Browser group", icon: null, depth: 2 },
  ];

  it("moves through the places, skipping headings and stopping at the ends", () => {
    const update = options[1]!.target!;
    expect(stepTarget(options, update, 1)).toEqual(options[3]!.target);
    expect(stepTarget(options, options[3]!.target!, 1)).toEqual(options[4]!.target);
    expect(stepTarget(options, options[4]!.target!, 1)).toEqual(options[4]!.target);
    expect(stepTarget(options, update, -1)).toEqual(update);
  });

  it("starts from the first place when the chosen one isn't listed", () => {
    expect(stepTarget(options, { kind: "update", vault: 3, entry: "x" }, 1)).toEqual(options[1]!.target);
  });

  it("compares places by what they are", () => {
    expect(sameTarget({ kind: "newEntry", vault: 0, group: null }, { kind: "newEntry", vault: 0, group: null })).toBe(true);
    expect(sameTarget({ kind: "newEntry", vault: 0, group: null }, { kind: "newEntry", vault: 1, group: null })).toBe(false);
    expect(sameTarget({ kind: "update", vault: 0, entry: "a" }, { kind: "newEntry", vault: 0, group: "a" })).toBe(false);
  });

  it("sets the Browser group to be made apart from its name", () => {
    expect(splitNewGroup("Browser (new Group)")).toEqual({ name: "Browser", isNew: true });
    expect(splitNewGroup("SaaS")).toEqual({ name: "SaaS", isNew: false });
  });
});

describe("a Group to delete", () => {
  const node = (id: string, totalEntries: number, children: GroupNode[] = []): GroupNode => ({
    id,
    name: id,
    entryCount: 0,
    totalEntries,
    isRecycleBin: false,
    inRecycleBin: false,
    isBrowserGroup: false,
    icon: { kind: "standard", number: 48 },
    children,
  });
  const tree = node("root", 9, [node("old", 6, [node("older", 2, [node("oldest", 0)])]), node("empty", 0)]);

  it("is found anywhere in the tree", () => {
    expect(findGroup(tree, "oldest")?.id).toBe("oldest");
    expect(findGroup(tree, "missing")).toBeNull();
  });

  it("says what is in it", () => {
    expect(contentsLine(findGroup(tree, "old")!)).toBe("6 Entries and 2 Groups inside");
    expect(contentsLine(findGroup(tree, "older")!)).toBe("2 Entries and 1 Group inside");
    expect(contentsLine(findGroup(tree, "empty")!)).toBe("Empty");
  });
});
