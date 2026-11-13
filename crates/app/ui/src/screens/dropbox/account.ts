// What disconnecting the Dropbox account asks first, from General Settings and from Open from
// Dropbox A alike.

import type { VaultSummary } from "@/bindings";

/** The Vaults open from Dropbox, locked or not: they can't sync while Dropbox is disconnected. */
export function dropboxVaults(vaults: readonly VaultSummary[]): VaultSummary[] {
  return vaults.filter((vault) => vault.location.provider === "dropbox");
}

/** The confirmation before disconnecting, naming the Vaults open from Dropbox. */
export function disconnectConfirmation(open: readonly VaultSummary[]): { title: string; description: string } {
  const title = "Disconnect Dropbox?";
  if (open.length === 0) {
    return {
      title,
      description: "keefast forgets its access to your Dropbox. Vaults in Dropbox open again once you connect it again.",
    };
  }
  const names = new Intl.ListFormat("en", { type: "conjunction" }).format(open.map((vault) => vault.name));
  const which = open.length === 1 ? `${names} is open from Dropbox` : `${names} are open from Dropbox`;
  return {
    title,
    description: `${which}. Until you connect Dropbox again, saves stay on this machine, encrypted, and upload once you do.`,
  };
}
