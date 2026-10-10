// Where the user is: what the Entry list shows and the Entry selected. The backend keeps it
// (`snapshot.view`, so unlocking a Vault returns there); the UI shows a change at once and tells
// the backend, whose answer comes back with the next snapshot. Until it does, the view asked for
// wins over an older snapshot, so moving fast through the list doesn't flicker.

import { useCallback, useEffect, useSyncExternalStore } from "react";

import { api, errorMessage } from "@/api";
import type { EntryKey, Scope, View } from "@/bindings";
import { useWorkspace } from "@/api/queries";
import { toast } from "@/kit";

/** How long a view asked for wins over snapshots that don't show it yet. */
const PENDING_MS = 1500;

let pending: { view: View; at: number } | null = null;
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
    case "tag":
      return b.kind === "tag" && a.tag === b.tag;
    case "color":
      return b.kind === "color" && a.color === b.color;
  }
}

export function sameView(a: View, b: View): boolean {
  const sameEntry = a.entry === null ? b.entry === null : b.entry !== null && a.entry.vault === b.entry.vault && a.entry.entry === b.entry.entry;
  return sameEntry && sameScope(a.scope, b.scope);
}

/** Shows `view` now and tells the backend. */
export function showView(view: View) {
  pending = { view, at: Date.now() };
  notify();
  void api.setView(view).catch((error) => {
    pending = null;
    notify();
    toast(errorMessage(error), { icon: "error" });
  });
}

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
  const backend = snapshot?.view;
  useEffect(() => {
    if (!asked || !backend) return;
    if (sameView(asked.view, backend)) {
      pending = null;
      notify();
      return;
    }
    const timer = setTimeout(
      () => {
        if (pending === asked) {
          pending = null;
          notify();
        }
      },
      Math.max(0, asked.at + PENDING_MS - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [asked, backend]);
  return asked?.view ?? backend;
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
