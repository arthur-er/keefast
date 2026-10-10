// What the list and the detail both do to an Entry: copy a field, open its URL, delete it.

import { Trash2 } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { CopyTarget, EntryId, VaultId } from "@/bindings";
import { toast } from "@/kit";
import type { ShellActions } from "@/shell/state";

/** Copies a field, says so with the clipboard's countdown, and resolves whether it copied. */
export async function copyField(vault: VaultId, entry: EntryId, field: CopyTarget, what: string): Promise<boolean> {
  try {
    const copied = await api.copyField(vault, entry, field);
    // The toast counts down to the clipboard clearing, when it will.
    toast(copied.clearsInSeconds !== null ? <>{what} copied<span className="subtle"> · clipboard clears in</span></> : `${what} copied`, {
      icon: "clipboard",
      seconds: copied.clearsInSeconds ?? undefined,
    });
    return true;
  } catch (error) {
    toast(errorMessage(error), { icon: "info" });
    return false;
  }
}

export function openUrl(vault: VaultId, entry: EntryId) {
  void api.openEntryUrl(vault, entry).catch((error) => toast(errorMessage(error), { icon: "error" }));
}

/** Moves the Entry to the Recycle Bin, or deletes it for good after asking. */
export async function deleteEntry(vault: VaultId, entry: EntryId, shell: ShellActions) {
  try {
    const detail = await api.getEntry(vault, entry);
    const remove = async () => {
      try {
        await api.deleteEntry(vault, entry);
        toast(detail.deletesForGood ? `Deleted “${detail.title}”` : "Moved to the Recycle Bin", { icon: Trash2 });
      } catch (error) {
        toast(errorMessage(error), { icon: "error" });
      }
    };
    if (!detail.deletesForGood) return void (await remove());
    shell.confirm({
      title: `Delete “${detail.title}” for good?`,
      description: detail.inRecycleBin
        ? "It is in the Recycle Bin already. Deleting it can't be undone."
        : "This Vault has no Recycle Bin, so deleting it can't be undone.",
      confirmLabel: "Delete",
      danger: true,
      onConfirm: remove,
    });
  } catch (error) {
    toast(errorMessage(error), { icon: "error" });
  }
}
