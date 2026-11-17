// A Group of a Vault, picked from a menu of its Groups in tree order: for the Browser group and
// the Agent group. The Recycle Bin and what is in it can't be picked.

import { Check as CheckIcon, ChevronsUpDown, Folder, Trash2 } from "lucide-react";
import { DropdownMenu as MenuPrimitive } from "radix-ui";

import type { GroupId, GroupNode, VaultId } from "@/bindings";
import { useGroups } from "@/api/queries";
import { GroupIcon, Menu, MenuContent, MenuLabel, MenuSeparator, MenuTrigger } from "@/kit";

/** The Groups that can be picked, in tree order, with their depth. */
export function groupChoices(root: GroupNode): { group: GroupNode; depth: number }[] {
  const out: { group: GroupNode; depth: number }[] = [];
  const walk = (group: GroupNode, depth: number) => {
    if (group.inRecycleBin) return;
    out.push({ group, depth });
    group.children.forEach((child) => walk(child, depth + 1));
  };
  walk(root, 0);
  return out;
}

export function GroupPicker({
  vault,
  label,
  value,
  valueName,
  placeholder,
  menuLabel,
  binNote,
  onPick,
}: {
  vault: VaultId;
  /** What the Group is for, such as "Browser group". */
  label: string;
  value: GroupId | null;
  valueName: string | null;
  /** Shown while no Group is picked. */
  placeholder: string;
  menuLabel: string;
  /** Why the Recycle Bin can't be picked. */
  binNote: string;
  onPick: (group: GroupId) => void;
}) {
  const groups = useGroups(vault).data;
  const choices = groups ? groupChoices(groups) : [];
  const bin = groups?.children.find((group) => group.isRecycleBin);
  const chosen = choices.find(({ group }) => group.id === value)?.group;
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="pick" aria-label={`${label}: ${valueName ?? "not set"}`}>
          {chosen ? <GroupIcon icon={chosen.icon} size={14} className="ico-sm" /> : <Folder className="ico-sm" />}
          <span className="truncate">{valueName ?? placeholder}</span>
          <ChevronsUpDown className="chev ico-sm" />
        </button>
      </MenuTrigger>
      <MenuContent align="end" className="max-h-[360px] w-[240px] overflow-y-auto">
        <MenuLabel>{menuLabel}</MenuLabel>
        <MenuPrimitive.RadioGroup value={value ?? ""} onValueChange={onPick}>
          {choices.map(({ group, depth }) => (
            <MenuPrimitive.RadioItem
              key={group.id}
              value={group.id}
              className="menu-item outline-none data-[highlighted]:bg-bg-active"
              style={{ paddingLeft: 8 + depth * 12 }}
            >
              <GroupIcon icon={group.icon} />
              <span className="truncate">{group.name}</span>
              <MenuPrimitive.ItemIndicator className="chk">
                <CheckIcon />
              </MenuPrimitive.ItemIndicator>
            </MenuPrimitive.RadioItem>
          ))}
        </MenuPrimitive.RadioGroup>
        {bin && (
          <>
            <MenuSeparator />
            <MenuPrimitive.Item disabled className="menu-item outline-none data-[disabled]:text-fg-2">
              <Trash2 /> Recycle Bin
            </MenuPrimitive.Item>
            <div className="menu-note">{binNote}</div>
          </>
        )}
      </MenuContent>
    </Menu>
  );
}
