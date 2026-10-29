// Vault browser C's sidebar: the Vault switcher, the shown Vault's Entries, Groups, tags and
// colours, the other Vaults, the Recycle Bin, and the footer's Open / New, commands, settings
// and lock.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Check, ChevronsUpDown, ChevronUp, CircleHelp, Ellipsis, Folder, FolderOpen, Hash, LayoutGrid, LayoutList, Lock,
  MousePointerClick, Plus, Settings, Settings2, Trash2,
} from "lucide-react";

import { api, errorMessage } from "@/api";
import type { Color, GroupNode, Scope, VaultSummary } from "@/bindings";
import { useEntries, useGroups, useWorkspace } from "@/api/queries";
import { colorVar, Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, toast, Tooltip } from "@/kit";
import { cn } from "@/lib/utils";
import { GroupContextMenu, useDeleteGroupCommand } from "@/screens/dialogs/GroupActions";

import { useCurrentVault } from "./hooks";
import { useShell } from "./state";
import { sameScope, useShowScope, useView } from "./view";
import { VaultMark } from "./VaultMark";
import { storageLine, vaultIcon, vaultStatus } from "./vaultStatus";

export const COLORS: readonly Color[] = ["red", "orange", "yellow", "green", "blue", "violet"];
export const COLOR_NAMES: Record<Color, string> = {
  red: "Red", orange: "Orange", yellow: "Yellow", green: "Green", blue: "Blue", violet: "Violet",
};

/** How many tags show before "n more". */
const TAGS_SHOWN = 6;

export function Sidebar() {
  const vault = useCurrentVault();
  useDeleteGroupCommand();
  return (
    <aside className="vb-side" aria-label="Vaults">
      <div className="pt-1">{vault && <VaultSwitcher current={vault} />}</div>
      <div className="vb-side-scroll pt-1.5">
        {vault?.state === "unlocked" ? (
          <OpenVaultNav vault={vault} />
        ) : (
          <div className="nav-section">
            <div className="nav-item is-selected">
              <Lock /> <span>Locked</span>
            </div>
          </div>
        )}
        <OtherVaults current={vault} />
      </div>
      {vault?.state === "unlocked" && vault.recycleBin && <RecycleBin vault={vault} bin={vault.recycleBin} />}
      <SideFoot />
    </aside>
  );
}

function NavItem({
  scope,
  icon,
  label,
  count,
  depth = 0,
  className,
  extra,
}: {
  scope: Scope;
  icon: React.ReactNode;
  label: React.ReactNode;
  count?: number | string | null;
  depth?: number;
  className?: string;
  extra?: React.ReactNode;
}) {
  const view = useView();
  const showScope = useShowScope();
  const { state, actions } = useShell();
  // A settings page shows nothing of the scope, so no row is the one shown.
  const selected = view && state.page.kind === "browser" ? sameScope(view.scope, scope) : false;
  return (
    <button
      type="button"
      className={cn("nav-item w-full text-left", selected && "is-selected", className)}
      style={{ paddingLeft: 8 + depth * 16 }}
      aria-current={selected ? "page" : undefined}
      onClick={() => {
        actions.showBrowser();
        showScope(scope);
      }}
    >
      {icon}
      <span className="truncate">{label}</span>
      {extra}
      {count !== undefined && count !== null && count !== "" && <span className="count">{count}</span>}
    </button>
  );
}

function VaultSwitcher({ current }: { current: VaultSummary }) {
  const snapshot = useWorkspace().data;
  const showScope = useShowScope();
  const { actions } = useShell();
  const show = (scope: Scope) => {
    actions.showBrowser();
    showScope(scope);
  };
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="C-switch" aria-label={`Vault: ${current.name}`}>
          <span className="vb-vault-ico">
            <VaultMark vault={current} />
          </span>
          <span className="vstack min-w-0 gap-0">
            <span className="name truncate">{current.name}</span>
            <span className="file truncate">{storageLine(current)}</span>
          </span>
          <ChevronsUpDown />
        </button>
      </MenuTrigger>
      <MenuContent align="start" className="w-[240px]">
        <MenuLabel>Open Vaults</MenuLabel>
        {snapshot?.vaults.map((vault) => (
          <MenuItem key={vault.id} icon={<VaultMark vault={vault} />} onSelect={() => show({ kind: "vault", vault: vault.id })}>
            <span className={cn("truncate", vault.state === "locked" && "text-fg-1")}>{vault.name}</span>
            <span className="sub">{vaultStatus(vault).short}</span>
            {vault.id === current.id && <Check className="vb-menu-check" />}
          </MenuItem>
        ))}
        <MenuSeparator />
        <MenuItem icon={<LayoutGrid />} onSelect={() => show({ kind: "allVaults" })}>
          All Entries across Vaults
        </MenuItem>
        <MenuItem icon={<FolderOpen />} shortcut="Ctrl O" onSelect={() => actions.showOpenVault()}>
          Open Vault file…
        </MenuItem>
        <MenuItem icon={<Plus />} onSelect={() => actions.showOpenVault("new")}>
          New Vault…
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={<Settings2 />} disabled={current.state !== "unlocked"} onSelect={() => actions.openVaultSettings(current.id)}>
          Vault Settings
        </MenuItem>
        <MenuItem icon={<Lock />} shortcut="Ctrl L" onSelect={() => void api.lockAll().catch(showError)}>
          Lock all Vaults
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}

function OpenVaultNav({ vault }: { vault: VaultSummary }) {
  const groups = useGroups(vault.id).data;
  // The Vault's Entries, to count its tags and colours as the sidebar shows them.
  const entries = useEntries({ scope: { kind: "vault", vault: vault.id }, search: "", sort: "title" }).data;
  const bin = groups ? findGroup(groups, (group) => group.isRecycleBin) : null;
  const allCount = groups ? groups.totalEntries - (bin?.totalEntries ?? 0) : null;
  const live = useMemo(() => (entries ?? []).filter((entry) => !entry.inRecycleBin), [entries]);
  const tagCounts = useMemo(() => {
    const counts = new Map<string, number>();
    live.forEach((entry) => entry.tags.forEach((tag) => counts.set(tag, (counts.get(tag) ?? 0) + 1)));
    return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [live]);
  const [allTags, setAllTags] = useState(false);
  const tags = allTags ? tagCounts : tagCounts.slice(0, TAGS_SHOWN);

  return (
    <>
      <div className="nav-section">
        <NavItem scope={{ kind: "vault", vault: vault.id }} icon={<LayoutList />} label="All Entries" count={allCount} />
      </div>
      <div className="nav-section">
        <div className="nav-head">
          Groups
          {groups && <NewGroupButton vault={vault.id} parent={groups.id} />}
        </div>
        <div className="vb-tree">{groups && <GroupTree vault={vault.id} group={groups} depth={0} />}</div>
      </div>
      {tagCounts.length > 0 && (
        <div className="nav-section">
          <div className="nav-head">Tags</div>
          {tags.map(([tag, count]) => (
            <NavItem key={tag} scope={{ kind: "tag", tag }} icon={<Hash />} label={tag} count={count} />
          ))}
          {tagCounts.length > TAGS_SHOWN && (
            <button type="button" className="nav-item vb-more w-full text-left" onClick={() => setAllTags(!allTags)}>
              {allTags ? <ChevronUp /> : <Ellipsis />}
              <span>{allTags ? "Show fewer" : `${tagCounts.length - TAGS_SHOWN} more`}</span>
            </button>
          )}
        </div>
      )}
      <ColorSection />
    </>
  );
}

function GroupTree({ vault, group, depth }: { vault: number; group: GroupNode; depth: number }) {
  return (
    <>
      {group.children
        .filter((child) => !child.isRecycleBin)
        .map((child) => (
          <div key={child.id}>
            <GroupItem vault={vault} group={child} depth={depth} />
            <GroupTree vault={vault} group={child} depth={depth + 1} />
          </div>
        ))}
    </>
  );
}

function GroupItem({ vault, group, depth }: { vault: number; group: GroupNode; depth: number }) {
  const { state, actions } = useShell();
  const renaming = state.renamingGroup === `${vault}:${group.id}`;
  if (renaming) {
    return (
      <div className="nav-item is-selected" style={{ paddingLeft: 8 + depth * 16 }}>
        <Folder />
        <RenameField
          initial={group.name}
          onDone={(name) => {
            actions.renameGroup(null);
            if (name && name !== group.name) void api.renameGroup(vault, group.id, name).catch(showError);
          }}
        />
      </div>
    );
  }
  const mark = group.isBrowserGroup ? (
    <Tooltip content="Browser group: sign-ins captured by the Companion extension are saved here">
      <span className="vb-bmark">
        <MousePointerClick />
      </span>
    </Tooltip>
  ) : null;
  return (
    <GroupContextMenu vault={vault} group={group}>
      <div onDoubleClick={() => actions.renameGroup(`${vault}:${group.id}`)}>
        <NavItem
          scope={{ kind: "group", vault, group: group.id }}
          icon={<Folder />}
          label={group.name}
          count={group.totalEntries || ""}
          depth={depth}
          extra={mark}
        />
      </div>
    </GroupContextMenu>
  );
}

function RenameField({ initial, onDone }: { initial: string; onDone: (name: string | null) => void }) {
  const [name, setName] = useState(initial);
  const field = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  const finish = (value: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(value);
  };
  useEffect(() => field.current?.select(), []);
  return (
    <input
      ref={field}
      className="vb-rename"
      aria-label="Group name"
      value={name}
      onChange={(event) => setName(event.target.value)}
      onBlur={() => finish(name.trim())}
      onKeyDown={(event) => {
        if (event.key === "Enter") finish(name.trim());
        if (event.key === "Escape") finish(null);
      }}
    />
  );
}

function NewGroupButton({ vault, parent }: { vault: number; parent: string }) {
  const { actions } = useShell();
  return (
    <Tooltip content="New Group">
      <button
        type="button"
        className="btn ghost sm icon !h-5 !w-5"
        aria-label="New Group"
        onClick={async () => {
          try {
            const group = await api.createGroup(vault, parent, "New Group");
            actions.renameGroup(`${vault}:${group}`);
          } catch (error) {
            showError(error);
          }
        }}
      >
        <Plus />
      </button>
    </Tooltip>
  );
}

function ColorSection() {
  const view = useView();
  const showScope = useShowScope();
  const { actions } = useShell();
  return (
    <div className="nav-section">
      <div className="nav-head h-[30px] pr-0">
        Colours
        <div className="vb-colors ml-auto p-0">
          {COLORS.map((color) => {
            const scope: Scope = { kind: "color", color };
            const selected = view ? sameScope(view.scope, scope) : false;
            return (
              <Tooltip key={color} content={COLOR_NAMES[color]}>
                <button
                  type="button"
                  className={cn("vb-color p-0", selected && "is-selected")}
                  aria-label={`${COLOR_NAMES[color]} Entries`}
                  aria-pressed={selected}
                  onClick={() => {
                    actions.showBrowser();
                    showScope(scope);
                  }}
                >
                  <span className="dot" style={{ background: colorVar(color) }} />
                </button>
              </Tooltip>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function OtherVaults({ current }: { current: VaultSummary | undefined }) {
  const snapshot = useWorkspace().data;
  const others = (snapshot?.vaults ?? []).filter((vault) => vault.id !== current?.id);
  if (others.length === 0) return null;
  return (
    <div className="nav-section">
      <div className="nav-head">Other Vaults</div>
      {others.map((vault) => {
        const Icon = vaultIcon(vault);
        const status = vaultStatus(vault);
        return (
          <NavItem
            key={vault.id}
            scope={{ kind: "vault", vault: vault.id }}
            className={cn("vb-nav-vault", vault.state === "locked" && "vb-nav-locked")}
            icon={
              <span className="vb-vault-ico">
                <Icon />
              </span>
            }
            label={vault.name}
            extra={
              <span className="vb-sync">
                {vault.state !== "locked" && <span className={cn("status-dot", dotClass(status.tone))} />}
                {status.short}
              </span>
            }
          />
        );
      })}
    </div>
  );
}

function RecycleBin({ vault, bin }: { vault: VaultSummary; bin: string }) {
  const groups = useGroups(vault.id).data;
  const node = groups ? findGroup(groups, (group) => group.id === bin) : null;
  return (
    <div className="vb-side-bottom">
      <NavItem scope={{ kind: "group", vault: vault.id, group: bin }} icon={<Trash2 />} label="Recycle Bin" count={node?.totalEntries || ""} />
    </div>
  );
}

function SideFoot() {
  const { actions } = useShell();
  return (
    <div className="vb-side-foot">
      <button type="button" className="btn ghost sm" onClick={() => actions.showOpenVault()}>
        <Plus /> Open / New
      </button>
      <span className="spacer" />
      <Tooltip content="Commands" shortcut="Ctrl K">
        <button type="button" className="btn ghost sm icon" aria-label="Commands" onClick={actions.togglePalette}>
          <CircleHelp />
        </button>
      </Tooltip>
      <Tooltip content="Settings" shortcut="Ctrl ,">
        <button type="button" className="btn ghost sm icon" aria-label="Settings" onClick={() => actions.openSettings()}>
          <Settings />
        </button>
      </Tooltip>
      <Tooltip content="Lock all Vaults" shortcut="Ctrl L">
        <button type="button" className="btn ghost sm icon" aria-label="Lock all Vaults" onClick={() => void api.lockAll().catch(showError)}>
          <Lock />
        </button>
      </Tooltip>
    </div>
  );
}

export function dotClass(tone: ReturnType<typeof vaultStatus>["tone"]): string {
  return tone === "ok" ? "" : tone;
}

export function findGroup(group: GroupNode, test: (group: GroupNode) => boolean): GroupNode | null {
  if (test(group)) return group;
  for (const child of group.children) {
    const found = findGroup(child, test);
    if (found) return found;
  }
  return null;
}

/** The Groups from the top Group down to `id`, or `[]` when it isn't in the tree. */
export function groupPath(group: GroupNode, id: string): GroupNode[] {
  if (group.id === id) return [group];
  for (const child of group.children) {
    const path = groupPath(child, id);
    if (path.length) return [group, ...path];
  }
  return [];
}

function showError(error: unknown) {
  toast(errorMessage(error), { icon: "error" });
}
