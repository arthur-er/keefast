// Vault settings C: one Vault's own settings, kept in its Vault file (ADR 0005): its Vault file
// (saving and locking it), how it answers the Companion extension (Default access policy,
// Browser group), its Pairings, and its master key and encryption. Same page as General
// Settings C; leaves when the Vault locks.

import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { Check as CheckIcon, ChevronsUpDown, Folder, Globe, Lock, PlugZap, Save, Shield, Trash2 } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { AccessPolicy, GroupNode, VaultId, VaultSettings, VaultSummary } from "@/bindings";
import { useGroups, useVaultSettings, useWorkspace } from "@/api/queries";
import { Button, Menu, MenuContent, MenuLabel, MenuSeparator, MenuTrigger, Popover, PopoverContent, PopoverTrigger, Segmented, toast } from "@/kit";
import { DropdownMenu as MenuPrimitive } from "radix-ui";
import { cn } from "@/lib/utils";
import { useShell } from "@/shell/state";
import { vaultStatus } from "@/shell/vaultStatus";

import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection } from "./SettingsLayout";
import { KEY_SEARCH, VaultKeySection } from "./VaultKeySection";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

const POLICIES: readonly { value: AccessPolicy; label: string }[] = [
  { value: "ask", label: "Ask me" },
  { value: "alwaysAllow", label: "Always allow" },
  { value: "alwaysDeny", label: "Always deny" },
];

const POLICY_NOTE = "When a site asks for an Entry you haven't decided on for it. A decision remembered on the Entry always comes first.";

export function VaultSettingsPage({ vault, section }: { vault: VaultId; section?: string }) {
  const snapshot = useWorkspace().data;
  const summary = snapshot?.vaults.find((v) => v.id === vault);
  const unlocked = summary?.state === "unlocked";
  const settings = useVaultSettings(unlocked ? vault : undefined).data;
  const { actions } = useShell();
  const [changingKey, setChangingKey] = useState(section === "key");
  // Asked for again while the page shows (the palette's "Change master key and encryption…").
  useEffect(() => {
    if (section === "key") setChangingKey(true);
  }, [section]);
  // Once closed, the card is no longer asked for: back on the page (after a lock), the row shows.
  const closeKey = useCallback(() => {
    setChangingKey(false);
    if (section === "key") actions.openVaultSettings(vault);
  }, [section, actions, vault]);

  // A Vault's settings go when it locks or closes, as in the GPUI app.
  const gone = snapshot !== undefined && !unlocked;
  useEffect(() => {
    if (gone) actions.showBrowser();
  }, [gone, actions]);

  const sections = useMemo<SettingsSection[]>(() => {
    if (!settings || !summary) return [];
    return [
      {
        id: "general",
        name: "General",
        cards: [[{
          id: "file",
          search: `Vault file ${settings.location.description} path location kdbx disk save lock status`,
          render: () => <VaultFileRow settings={settings} summary={summary} />,
        }]],
      },
      {
        id: "browser",
        name: "Browser access",
        desc: "How this Vault answers the Companion extension.",
        cards: [[
          {
            id: "policy",
            search: `Default access policy ${POLICY_NOTE} ask allow deny site extension companion`,
            render: () => (
              <SettingRow label="Default access policy" desc={POLICY_NOTE}>
                <Segmented
                  label="Default access policy"
                  value={settings.defaultAccessPolicy}
                  options={POLICIES}
                  onValueChange={(policy) => void api.setDefaultAccessPolicy(vault, policy).catch(fail)}
                />
              </SettingRow>
            ),
          },
          {
            id: "browserGroup",
            search: "Browser group Where sign-ins captured in the browser are saved as Entries by default save logins sign-ins group companion",
            render: () => (
              <SettingRow label="Browser group" desc="Where sign-ins captured in the browser are saved as Entries by default.">
                <BrowserGroupPicker vault={vault} settings={settings} />
              </SettingRow>
            ),
          },
        ]],
      },
      {
        id: "pairings",
        name: "Paired browsers",
        count: settings.pairings.length,
        desc: "These browsers can ask this Vault for Entries while it is open.",
        cards: [
          settings.pairings.length > 0
            ? settings.pairings.map((name) => ({
                id: `pairing-${name}`,
                search: `${name} pairing paired browser revoke`,
                render: () => <PairingRow vault={vault} name={name} />,
              }))
            : [{
                id: "none",
                search: "No browser is paired with this Vault pairing",
                render: () => (
                  <div className="set-row">
                    <div className="txt">
                      <div className="desc mt-0 flex items-center gap-2">
                        <PlugZap className="ico-sm" /> No browser is paired with this Vault.
                      </div>
                    </div>
                  </div>
                ),
              }],
        ],
      },
      {
        id: "security",
        name: "Security",
        desc: "What it takes to unlock this Vault.",
        cards: [[{
          id: "key",
          search: KEY_SEARCH,
          pinned: changingKey,
          render: () => (
            <VaultKeySection vault={vault} settings={settings} open={changingKey} onOpen={() => setChangingKey(true)} onClose={closeKey} />
          ),
        }]],
      },
    ];
  }, [settings, summary, vault, changingKey, closeKey]);

  if (!summary || !unlocked) return null;
  return (
    <SettingsLayout
      crumbs={
        <>
          <span className="truncate">{summary.name}</span> <CrumbSep /> <b>Vault settings</b>
        </>
      }
      searchLabel="Search Vault settings"
      title="Vault settings"
      sub={
        <>
          Saved in the Vault file, so they travel with it. For every Vault, see{" "}
          <button type="button" onClick={() => actions.openSettings()}>
            General Settings
          </button>
          .
        </>
      }
      sections={sections}
      tip="Changes are saved to the Vault file."
      emptyLabel="Vault settings"
      // "key" opens the change-key card, in the Security section.
      section={section === "key" ? "security" : section}
    />
  );
}

/** The Vault file: where it is, where it stands, and saving and locking it. */
function VaultFileRow({ settings, summary }: { settings: VaultSettings; summary: VaultSummary }) {
  const status = vaultStatus(summary);
  const tone = status.tone === "failed" ? "danger" : status.tone === "warn" || status.tone === "busy" ? status.tone : "";
  return (
    <SettingRow label="Vault file" desc={<span title={settings.location.description}><Hl>{settings.location.description}</Hl></span>} className="vs-file">
      <span className={cn("vs-status", tone)}>
        <span className="status-dot" />
        {status.text}
      </span>
      <Button size="sm" variant="ghost" disabled={summary.saving} onClick={() => void api.saveVault(summary.id).catch(fail)}>
        <Save /> Save
      </Button>
      <Button size="sm" variant="ghost" onClick={() => void api.lockVault(summary.id).catch(fail)}>
        <Lock /> Lock
      </Button>
    </SettingRow>
  );
}

/** The Groups a Browser group can be, in tree order, with their depth; the Recycle Bin and
 * what is in it can't be. */
export function browserGroupChoices(root: GroupNode): { group: GroupNode; depth: number }[] {
  const out: { group: GroupNode; depth: number }[] = [];
  const walk = (group: GroupNode, depth: number) => {
    if (group.inRecycleBin) return;
    out.push({ group, depth });
    group.children.forEach((child) => walk(child, depth + 1));
  };
  walk(root, 0);
  return out;
}

function BrowserGroupPicker({ vault, settings }: { vault: VaultId; settings: VaultSettings }) {
  const groups = useGroups(vault).data;
  const choices = groups ? browserGroupChoices(groups) : [];
  const bin = groups?.children.find((group) => group.isRecycleBin);
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="pick" aria-label={`Browser group: ${settings.browserGroupName ?? "not set"}`}>
          <Folder className="ico-sm" />
          <span className="truncate">{settings.browserGroupName ?? "Made on the first save"}</span>
          <ChevronsUpDown className="chev ico-sm" />
        </button>
      </MenuTrigger>
      <MenuContent align="end" className="max-h-[360px] w-[240px] overflow-y-auto">
        <MenuLabel>Save browser sign-ins here</MenuLabel>
        <MenuPrimitive.RadioGroup
          value={settings.browserGroup ?? ""}
          onValueChange={(group) => void api.setBrowserGroup(vault, group).catch(fail)}
        >
          {choices.map(({ group, depth }) => (
            <MenuPrimitive.RadioItem
              key={group.id}
              value={group.id}
              className="menu-item outline-none data-[highlighted]:bg-bg-active"
              style={{ paddingLeft: 8 + depth * 12 }}
            >
              <Folder />
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
            <div className="menu-note">Entries from the browser can't be saved in the Recycle Bin.</div>
          </>
        )}
      </MenuContent>
    </Menu>
  );
}

/** A paired browser, revoked once the user confirms in a popover. */
function PairingRow({ vault, name }: { vault: VaultId; name: string }) {
  const [open, setOpen] = useState(false);
  const revoke = () => {
    setOpen(false);
    api.revokePairing(vault, name).then(() => toast(`Pairing “${name}” revoked`, { icon: "check" }), fail);
  };
  return (
    <div className="set-row pair">
      <div className="pair-ico">{/brave/i.test(name) ? <Shield className="ico-sm" /> : <Globe className="ico-sm" />}</div>
      <div className="txt">
        <div className="lbl truncate">
          <Hl>{name}</Hl>
        </div>
      </div>
      <div className="ctl">
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button size="sm">Revoke</Button>
          </PopoverTrigger>
          <PopoverContent
            side="top"
            align="end"
            className="confirm-pop"
            onEscapeKeyDown={(event) => {
              // Escape closes the popover, not the page under it.
              event.preventDefault();
              setOpen(false);
            }}
          >
            <ConfirmRevoke name={name} onCancel={() => setOpen(false)} onRevoke={revoke} />
          </PopoverContent>
        </Popover>
      </div>
    </div>
  );
}

function ConfirmRevoke({ name, onCancel, onRevoke }: { name: string; onCancel: () => void; onRevoke: () => void }): ReactNode {
  return (
    <>
      <h4>Revoke the Pairing “{name}”?</h4>
      <p>It must pair again to fill Entries.</p>
      <div className="hstack justify-end">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" variant="danger" autoFocus onClick={onRevoke}>
          Revoke
        </Button>
      </div>
    </>
  );
}
