// What the shell says about a Vault: its icon, and where it stands with its Vault file (the
// toolbar's status, the Vault switcher, Other Vaults).

import { Cloud, HardDrive, Lock, type LucideIcon } from "lucide-react";

import type { VaultSummary } from "@/bindings";

/** The status dot's look: `ok` green, `busy` pulsing, `warn` amber, `failed` red. */
export type StatusTone = "ok" | "busy" | "warn" | "failed" | "locked";

export interface VaultStatus {
  tone: StatusTone;
  /** As the toolbar says it: "All changes saved", "Unsaved changes", "Synced"… */
  text: string;
  /** As the sidebar says it: "Saved", "Unsaved"… */
  short: string;
}

export function vaultStatus(vault: VaultSummary): VaultStatus {
  if (vault.state === "locked") return { tone: "locked", text: "Locked", short: "Locked" };
  if (vault.state === "locking") return { tone: "busy", text: "Locking…", short: "Locking…" };
  if (vault.saving) return { tone: "busy", text: "Saving…", short: "Saving…" };
  if (vault.saveFailed) return { tone: "failed", text: "Save failed", short: "Save failed" };
  if (vault.unsavedChanges) return { tone: "warn", text: "Unsaved changes", short: "Unsaved" };
  switch (vault.syncStatus) {
    case "synced":
      return { tone: "ok", text: "Synced", short: "Synced" };
    case "syncing":
      return { tone: "busy", text: "Syncing…", short: "Syncing…" };
    case "offline":
      return { tone: "warn", text: "Offline", short: "Offline" };
    case "error":
      return { tone: "failed", text: "Sync error", short: "Sync error" };
    case null:
      return { tone: "ok", text: "All changes saved", short: "Saved" };
  }
}

/** The status the toolbar shows: the shown Vault's, or else the first open Vault that needs
 * attention, or all saved. */
export function toolbarStatus(vaults: readonly VaultSummary[], shown: VaultSummary | undefined): VaultStatus {
  if (shown && shown.state !== "locked") return vaultStatus(shown);
  const statuses = vaults.filter((vault) => vault.state !== "locked").map(vaultStatus);
  return statuses.find((status) => status.tone !== "ok") ?? { tone: "ok", text: "All changes saved", short: "Saved" };
}

/** What the Vault switcher says under a Vault's name: "Local · Saved", "Dropbox · Synced". */
export function storageLine(vault: VaultSummary): string {
  if (vault.state === "locked") return "Locked";
  const status = vaultStatus(vault).short;
  return vault.location.provider === "dropbox" ? `Dropbox · ${status}` : `Local · ${status}`;
}

export function vaultIcon(vault: Pick<VaultSummary, "state" | "location">): LucideIcon {
  if (vault.state === "locked") return Lock;
  return vault.location.provider === "dropbox" ? Cloud : HardDrive;
}
