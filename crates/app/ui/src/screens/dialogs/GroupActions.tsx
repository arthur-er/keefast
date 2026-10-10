// Deleting a Group, from its right-click menu in the sidebar or the command palette while the
// list shows it. (The GPUI app did it from the Group's pane; Vault browser C has none.)

import type { ReactNode } from "react";
import { Pencil, Trash2 } from "lucide-react";

import type { GroupNode } from "@/bindings";
import { useWorkspace } from "@/api/queries";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/kit";
import { useAppCommand } from "@/shell/commands";
import { useKeysActive } from "@/shell/hooks";
import { useShell } from "@/shell/state";
import { useView } from "@/shell/view";

import { deleteGroup } from "./DeleteDialog";

/** A sidebar Group's right-click menu: rename it, delete it. */
export function GroupContextMenu({ vault, group, children }: { vault: number; group: GroupNode; children: ReactNode }) {
  const { actions } = useShell();
  const scope = useView()?.scope;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-[200px]">
        <ContextMenuItem icon={<Pencil />} onSelect={() => actions.renameGroup(`${vault}:${group.id}`)}>
          Rename
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem icon={<Trash2 />} danger onSelect={() => void deleteGroup(vault, group.id, actions, scope)}>
          Delete Group
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** "Delete Group" in the command palette, for the Group the list shows (not the Vault's top
 * Group or its Recycle Bin). */
export function useDeleteGroupCommand() {
  const { actions } = useShell();
  const scope = useView()?.scope;
  const vault = useWorkspace().data?.vaults.find((v) => scope?.kind === "group" && v.id === scope.vault);
  const group = scope?.kind === "group" ? scope.group : null;
  const enabled = !!vault && vault.state === "unlocked" && group !== null && group !== vault.rootGroup && group !== vault.recycleBin;
  useAppCommand(
    {
      id: "deleteGroup",
      title: "Delete Group",
      section: "Group",
      icon: Trash2,
      enabled,
      run: () => {
        if (enabled && vault && group) void deleteGroup(vault.id, group, actions, scope);
      },
    },
    useKeysActive(),
  );
}
