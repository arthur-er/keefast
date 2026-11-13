// Where the user is: what the Entry list shows and the Entry selected. The backend keeps it
// (`snapshot.view`, so unlocking a Vault returns there); the UI shows a change at once and tells
// the backend. Until a snapshot shows the change, the view asked for wins, so moving fast through
// the list doesn't flicker or jump back.
//
// "Shows the change" goes by revision, not by comparing views: a snapshot read before the change
// can show the very view asked for (moving down and straight back up), and the snapshot after it
// then shows the Entry passed through. So views are sent one at a time, in order (of those still
// waiting, only the latest), and `setView` answers with the revision snapshots show it from.

import { useCallback, useEffect, useSyncExternalStore } from "react";

import { api, errorMessage } from "@/api";
import type { EntryKey, Scope, View } from "@/bindings";
import { useWorkspace } from "@/api/queries";
import { toast } from "@/kit";

/** The view asked for and, once the backend has it, the revision snapshots show it from. */
interface Asked {
  view: View;
  revision: number | null;
}

let pending: Asked | null = null;
let sending = false;
const subscribers = new Set<() => void>();
const notify = () => subscribers.forEach((subscriber) => subscriber());

export function sameScope(a: Scope, b: Scope): boolean {
  switch (a.kind) {
    case "allVaults":
      return b.kind === "allVaults";
    case "vault":
      return b.kind === "vault" && a.vault === b.vault;
    case "group":
      return b.kind === "group" && a.vault === b.vault && a.group === b.group;
    case "smartList":
      return b.kind === "smartList" && a.vault === b.vault && a.list === b.list;
    case "tag":
      return b.kind === "tag" && a.tag === b.tag;
    case "color":
      return b.kind === "color" && a.color === b.color;
  }
}

/** Shows `view` now and tells the backend. */
export function showView(view: View) {
  pending = { view, revision: null };
  notify();
  void send();
}

/** Sends the view asked for, then the latest asked for meanwhile, so they arrive in order. */
async function send() {
  if (sending) return;
  sending = true;
  try {
    while (pending && pending.revision === null) {
      const asked = pending;
      try {
        const revision = await api.setView(asked.view);
        if (pending === asked) {
          pending = { view: asked.view, revision };
          notify();
        }
      } catch (error) {
        if (pending === asked) {
          pending = null;
          notify();
        }
        toast(errorMessage(error), { icon: "error" });
      }
    }
  } finally {
    sending = false;
  }
}

/** Whether a snapshot of `revision` was read after the backend took the view asked for. */
const caughtUp = (asked: Asked, revision: number | undefined) =>
  asked.revision !== null && revision !== undefined && revision >= asked.revision;

/** The view to show: the one asked for while the backend catches up, else the snapshot's. */
export function useView(): View | undefined {
  const snapshot = useWorkspace().data;
  const asked = useSyncExternalStore(
    (subscriber) => {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    },
    () => pending,
  );
  const done = asked !== null && caughtUp(asked, snapshot?.revision);
  useEffect(() => {
    if (done && pending === asked) {
      pending = null;
      notify();
    }
  }, [done, asked]);
  // From then on the snapshot's view is shown, even if it's another one: something since (a
  // lock, a delete) moved the user.
  return asked && !done ? asked.view : snapshot?.view;
}

/** Shows a scope in the Entry list, with no Entry selected yet (the list selects its first). */
export function useShowScope() {
  return useCallback((scope: Scope) => showView({ scope, entry: null }), []);
}

/** Selects an Entry in the scope shown. */
export function useSelectEntry() {
  const view = useView();
  return useCallback(
    (entry: EntryKey | null) => {
      if (view) showView({ scope: view.scope, entry });
    },
    [view],
  );
}
