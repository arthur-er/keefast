// What Entry editor A edits: the selected Entry, or a new Entry in a Group. The shell's detail
// mode says the pane edits (`ShellState.detail === "edit"`); this says what. "Edit" on the
// selected Entry needs nothing here: with no new Entry asked for, the editor edits the selection.

import { useSyncExternalStore } from "react";

import { api, errorMessage } from "@/api";
import type { Color, GroupId, View, VaultId } from "@/bindings";
import { toast } from "@/kit";
import type { ShellActions } from "@/shell/state";

export interface NewEntryTarget {
  vault: VaultId;
  group: GroupId;
  /** The tag or colour the list shows, which the new Entry gets so it shows up there. */
  tags: string[];
  color: Color | null;
}

let pending: NewEntryTarget | null = null;
const subscribers = new Set<() => void>();

/** The new Entry asked for, until the editor takes it. */
export function useNewEntryRequest(): NewEntryTarget | null {
  return useSyncExternalStore(
    (notify) => {
      subscribers.add(notify);
      return () => subscribers.delete(notify);
    },
    () => pending,
  );
}

/** Takes the new Entry asked for, so a later "Edit" edits the selection again. */
export function takeNewEntryRequest(): NewEntryTarget | null {
  const taken = pending;
  pending = null;
  return taken;
}

/** Opens the editor on a new Entry in the Group new items go to for what the list shows. */
export async function startNewEntry(view: View | undefined, shell: ShellActions) {
  if (!view) return;
  try {
    const target = await api.groupForNewItems(view.scope);
    if (!target) return void toast("Pick a Vault first: the new Entry goes into the Vault shown.", { icon: "info" });
    pending = {
      ...target,
      tags: view.scope.kind === "tag" ? [view.scope.tag] : [],
      color: view.scope.kind === "color" ? view.scope.color : null,
    };
    subscribers.forEach((notify) => notify());
    shell.editEntry();
  } catch (error) {
    toast(errorMessage(error), { icon: "error" });
  }
}
