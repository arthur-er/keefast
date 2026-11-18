// Vault settings C: one Vault's own settings, kept in its Vault file (ADR 0005): its name, its
// Vault file (saving, locking and showing it), its Recycle Bin, how it answers the Companion extension (Default access policy,
// Browser group), its Pairings, its Agent group and Agent pairings, and its master key and encryption. Same page as General
// Settings C; leaves when the Vault locks.

import { useCallback, useEffect, useMemo, useState } from "react";
import { FolderOpen, Globe, Lock, PlugZap, Save, Shield } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { AccessPolicy, PairingView, VaultId, VaultSettings, VaultSummary } from "@/bindings";
import { useAgentSettings, useVaultSettings, useWorkspace } from "@/api/queries";
import { Button, Popover, PopoverTrigger, Segmented, Switch, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { useShell } from "@/shell/state";
import { vaultStatus } from "@/shell/vaultStatus";

import { agentsSection, gatewaysSection } from "./AgentSettings";
import { ConfirmPop } from "./ConfirmPop";
import { GroupPicker } from "./GroupPicker";
import { pairedText, usedText } from "./pairings";
import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection } from "./SettingsLayout";
import { KEY_SEARCH, VaultKeySection } from "./VaultKeySection";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

const POLICIES: readonly { value: AccessPolicy; label: string }[] = [
  { value: "ask", label: "Ask me" },
  { value: "alwaysAllow", label: "Always allow" },
  { value: "alwaysDeny", label: "Always deny" },
];

const POLICY_NOTE = "When a site asks for an Entry you haven't decided on for it. A decision remembered on the Entry always comes first.";

const NAME_NOTE = "Shown in the sidebar and when a browser asks for logins.";
const RECYCLE_NOTE = "Deleted Entries and Groups move to the Recycle Bin, so a delete can be undone.";

export function VaultSettingsPage({ vault, section }: { vault: VaultId; section?: string }) {
  const snapshot = useWorkspace().data;
  const summary = snapshot?.vaults.find((v) => v.id === vault);
  const unlocked = summary?.state === "unlocked";
  const settings = useVaultSettings(unlocked ? vault : undefined).data;
  const agents = useAgentSettings(unlocked ? vault : undefined).data;
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
        cards: [[
          {
            id: "name",
            search: `Name ${NAME_NOTE} title rename`,
            render: () => (
              <SettingRow label="Name" desc={NAME_NOTE}>
                <NameField key={settings.name} vault={vault} name={settings.name} />
              </SettingRow>
            ),
          },
          {
            id: "file",
            search: `Vault file ${settings.location.description} path location kdbx disk save lock status show in folder`,
            render: () => <VaultFileRow settings={settings} summary={summary} />,
          },
          {
            id: "recycle",
            search: `Recycle Bin ${RECYCLE_NOTE} trash delete undo`,
            render: () => (
              <SettingRow label="Recycle Bin" desc={RECYCLE_NOTE}>
                <Switch
                  label="Recycle Bin"
                  checked={settings.recycleBinEnabled}
                  onCheckedChange={(enabled) => void api.setRecycleBinEnabled(vault, enabled).catch(fail)}
                />
              </SettingRow>
            ),
          },
        ]],
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
            ? settings.pairings.map((pairing) => ({
                id: `pairing-${pairing.name}`,
                search: `${pairing.name} pairing paired browser revoke used`,
                render: () => <PairingRow vault={vault} pairing={pairing} />,
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
      ...(agents ? [agentsSection(vault, agents, summary?.name ?? ""), gatewaysSection(vault, agents)] : []),
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
  }, [settings, agents, summary, vault, changingKey, closeKey]);

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

/** The Vault's name, renamed when the field is left or Enter is pressed. Empty, the Vault is
 * named after its Vault file again. */
function NameField({ vault, name }: { vault: VaultId; name: string }) {
  const [value, setValue] = useState(name);
  const commit = () => {
    if (value.trim() === name) return setValue(name);
    api.renameVault(vault, value).catch((error) => {
      setValue(name);
      fail(error);
    });
  };
  return (
    <div className="input name-input">
      <input
        aria-label="Vault name"
        value={value}
        spellCheck={false}
        onChange={(event) => setValue(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") {
            // Escape puts the name back, not the page away.
            event.preventDefault();
            event.stopPropagation();
            setValue(name);
          }
        }}
      />
    </div>
  );
}

/** The Vault file: where it is, where it stands, saving and locking it, and showing it in its
 * folder when it is on this computer. */
function VaultFileRow({ settings, summary }: { settings: VaultSettings; summary: VaultSummary }) {
  const status = vaultStatus(summary);
  const tone = status.tone === "failed" ? "danger" : status.tone === "warn" || status.tone === "busy" ? status.tone : "";
  return (
    <SettingRow label="Vault file" desc={<span title={settings.location.description}><Hl>{settings.location.description}</Hl></span>} className="vs-file">
      <span className={cn("vs-status", tone)}>
        <span className="status-dot" />
        {status.text}
      </span>
      {settings.location.provider === "local" && (
        <Button size="sm" variant="ghost" onClick={() => void api.showVaultFileInFolder(summary.id).catch(fail)}>
          <FolderOpen /> <Hl>Show in folder</Hl>
        </Button>
      )}
      <Button size="sm" variant="ghost" disabled={summary.saving} onClick={() => void api.saveVault(summary.id).catch(fail)}>
        <Save /> Save
      </Button>
      <Button size="sm" variant="ghost" onClick={() => void api.lockVault(summary.id).catch(fail)}>
        <Lock /> Lock
      </Button>
    </SettingRow>
  );
}

function BrowserGroupPicker({ vault, settings }: { vault: VaultId; settings: VaultSettings }) {
  return (
    <GroupPicker
      vault={vault}
      label="Browser group"
      value={settings.browserGroup}
      valueName={settings.browserGroupName}
      placeholder="Made on the first save"
      menuLabel="Save browser sign-ins here"
      binNote="Entries from the browser can't be saved in the Recycle Bin."
      onPick={(group) => void api.setBrowserGroup(vault, group).catch(fail)}
    />
  );
}

/** A paired browser, with when it was last used and paired, revoked once the user confirms in
 * a popover. */
function PairingRow({ vault, pairing }: { vault: VaultId; pairing: PairingView }) {
  const { name } = pairing;
  const [open, setOpen] = useState(false);
  const used = usedText(pairing.lastUsed);
  const paired = pairedText(pairing.created);
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
        <div className="desc pair-meta">
          <span className={cn(used.live && "live")}>
            {used.live && <span className="status-dot" />}
            {used.text}
          </span>
          {paired && (
            <>
              <span>·</span>
              <span>{paired}</span>
            </>
          )}
        </div>
      </div>
      <div className="ctl">
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <Button size="sm">Revoke</Button>
          </PopoverTrigger>
          <ConfirmPop title={`Revoke the Pairing “${name}”?`} action="Revoke" onCancel={() => setOpen(false)} onConfirm={revoke}>
            It must pair again to fill Entries.
          </ConfirmPop>
        </Popover>
      </div>
    </div>
  );
}

