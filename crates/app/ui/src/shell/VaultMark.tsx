// A Vault's mark in the Vault switcher: Dropbox's own for a Vault whose Vault file is in Dropbox,
// locked or not, so it never looks like a local copy; else the shell's icon (a lock while locked).

import type { VaultSummary } from "@/bindings";
import { DropboxMark } from "@/screens/dropbox/DropboxMark";

import { vaultIcon } from "./vaultStatus";

export function VaultMark({ vault }: { vault: Pick<VaultSummary, "state" | "location"> }) {
  if (vault.location.provider === "dropbox") return <DropboxMark />;
  const Icon = vaultIcon(vault);
  return <Icon />;
}
