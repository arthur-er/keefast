// Small hooks the shell's parts share.

import { useEffect } from "react";

import type { Scope, VaultSummary, WorkspaceSnapshot } from "@/bindings";
import { useWorkspace } from "@/api/queries";

import { useShell } from "./state";
import { useView } from "./view";

/** Whether a dialog the backend asked for is up: a browser request, or a lock waiting. */
export function hasPendingRequest(snapshot: WorkspaceSnapshot | undefined): boolean {
  return !!(snapshot?.pendingLock || snapshot?.pendingPairing || snapshot?.pendingAccess || snapshot?.pendingLogin);
}

/** Whether the screens' keys work: not under an overlay or a browser request's dialog. */
export function useKeysActive(): boolean {
  const { state } = useShell();
  const snapshot = useWorkspace().data;
  return state.overlay === null && !hasPendingRequest(snapshot);
}

export const scopeVault = (scope: Scope): number | null =>
  scope.kind === "vault" || scope.kind === "group" || scope.kind === "smartList" ? scope.vault : null;

/** The Vault the sidebar is about: the one shown, or the last shown while the list shows a tag,
 * a colour or every Vault, or else the first unlocked. */
export function useCurrentVault(): VaultSummary | undefined {
  const snapshot = useWorkspace().data;
  const view = useView();
  const { state, actions } = useShell();
  const shown = view ? scopeVault(view.scope) : null;
  useEffect(() => {
    if (shown !== null) actions.rememberVault(shown);
  }, [shown, actions]);
  const vaults = snapshot?.vaults ?? [];
  const pick = (id: number | null) => (id === null ? undefined : vaults.find((vault) => vault.id === id));
  return pick(shown) ?? pick(state.lastVault) ?? vaults.find((vault) => vault.state === "unlocked") ?? vaults[0];
}
