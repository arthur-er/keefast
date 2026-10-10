// What the list and the detail both do to an Entry: copy a field, open its URL, delete it.

import { api, errorMessage } from "@/api";
import type { CopyTarget, EntryId, VaultId } from "@/bindings";
import { toast } from "@/kit";

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

/** Moves the Entry to the Recycle Bin, or deletes it for good after asking (Confirms A). */
export { deleteEntry } from "@/screens/dialogs/DeleteDialog";
