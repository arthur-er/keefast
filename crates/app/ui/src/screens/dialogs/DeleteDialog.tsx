// Deleting an Entry or a Group: moved to the Recycle Bin at once, or, when that deletes it for
// good (it is in the Recycle Bin already, or the Vault has none), after asking in Confirms A's
// delete dialog (`ShellState.overlay` kind `delete`).

import { useState } from "react";
import { Folder, Trash2 } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EntryId, GroupId, Scope, VaultId } from "@/bindings";
import { useEntry, useGroups } from "@/api/queries";
import { Button, toast } from "@/kit";
import { hostOf } from "@/lib/format";
import { EntryAvatar } from "@/screens/vault-browser/EntryList";
import type { DeleteOverlay, ShellActions } from "@/shell/state";
import { showView, useView } from "@/shell/view";

import { DialogFrame, Tile } from "./DialogFrame";
import { contentsLine, findGroup, groupsInside } from "./logic";

import "./dialogs.css";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

/** Deletes the Entry, asking first when that deletes it for good. */
export async function deleteEntry(vault: VaultId, entry: EntryId, shell: ShellActions) {
  try {
    const detail = await api.getEntry(vault, entry);
    if (detail.deletesForGood) return shell.openOverlay({ kind: "delete", vault, item: { kind: "entry", entry } });
    await api.deleteEntry(vault, entry);
    toast("Moved to the Recycle Bin", { icon: Trash2 });
  } catch (error) {
    fail(error);
  }
}

/** Deletes the Group and everything in it, asking first when that deletes it for good. */
export async function deleteGroup(vault: VaultId, group: GroupId, shell: ShellActions, scope?: Scope) {
  try {
    if (await api.deletesGroupForGood(vault, group)) {
      return shell.openOverlay({ kind: "delete", vault, item: { kind: "group", group } });
    }
    await removeGroup(vault, group, scope);
    toast("Moved to the Recycle Bin", { icon: Trash2 });
  } catch (error) {
    fail(error);
  }
}

/** Deletes the Group, and leaves it for its Vault if the list showed it or a Group in it. */
async function removeGroup(vault: VaultId, group: GroupId, scope?: Scope) {
  const tree = await api.listGroups(vault);
  const node = findGroup(tree, group);
  await api.deleteGroup(vault, group);
  if (scope?.kind !== "group" || scope.vault !== vault || !node) return;
  if ([node, ...groupsInside(node)].some((inside) => inside.id === scope.group)) {
    showView({ scope: { kind: "vault", vault }, entry: null });
  }
}

/** "Delete this Entry for good?" with the Entry, or the Group with what is in it. Enter deletes,
 * Escape cancels. */
export function DeleteDialog({ overlay, onClose }: { overlay: DeleteOverlay; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const scope = useView()?.scope;
  const { vault, item } = overlay;
  const entry = useEntry(vault, item.kind === "entry" ? item.entry : undefined).data;
  const tree = useGroups(item.kind === "group" ? vault : undefined).data;
  const group = item.kind === "group" && tree ? findGroup(tree, item.group) : null;
  const name = item.kind === "entry" ? entry?.title : group?.name;
  const inBin = item.kind === "entry" ? entry?.inRecycleBin : group?.inRecycleBin;

  const remove = async () => {
    setBusy(true);
    try {
      if (item.kind === "entry") await api.deleteEntry(vault, item.entry);
      else await removeGroup(vault, item.group, scope);
      toast(name ? <>Deleted <b>{name}</b></> : "Deleted", { icon: Trash2 });
    } catch (error) {
      fail(error);
    } finally {
      onClose();
    }
  };

  return (
    <DialogFrame
      role="alertdialog"
      width={440}
      tight
      lead={
        <Tile tone="danger">
          <Trash2 />
        </Tile>
      }
      title={item.kind === "entry" ? "Delete this Entry for good?" : "Delete this Group and everything in it for good?"}
      description="This cannot be undone."
      onEscape={onClose}
      actions={
        <>
          <Button onClick={onClose} shortcut="Esc">
            Cancel
          </Button>
          <Button variant="danger" autoFocus disabled={busy} onClick={() => void remove()} shortcut="↵">
            Delete for good
          </Button>
        </>
      }
    >
      <div className="dg-subject">
        {item.kind === "entry" ? (
          entry ? <EntryAvatar entry={entry} size={28} /> : <span className="dg-gico" />
        ) : (
          <span className="dg-gico">
            <Folder />
          </span>
        )}
        <div className="min-w-0">
          <div className="t truncate">{name ?? " "}</div>
          <div className="u truncate">
            {item.kind === "entry"
              ? entry && [entry.username, hostOf(entry.url)].filter(Boolean).join(" · ")
              : group && contentsLine(group)}
          </div>
        </div>
        {inBin !== undefined && (
          <span className="badge">
            <Trash2 className="ico-sm" />
            {inBin ? "Recycle Bin" : "No Recycle Bin"}
          </span>
        )}
      </div>
    </DialogFrame>
  );
}
