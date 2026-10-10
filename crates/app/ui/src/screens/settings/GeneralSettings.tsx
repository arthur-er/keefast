// General Settings C: the app config's machine-local preferences, on one long page with an
// "On this page" index and `/` search. Each change is kept as soon as it is made.

import { useCallback, useMemo, useState } from "react";
import {
  ArrowUpRight, Check as CheckIcon, ChevronsUpDown, CircleAlert, Cloud, LoaderCircle, LockOpen, RotateCw, Shield, TriangleAlert,
} from "lucide-react";
import { DropdownMenu as MenuPrimitive } from "radix-ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AppSettings, Theme, VaultSummary } from "@/bindings";
import { keys, usePlugins, useSettings, useVaultSettings, useWorkspace } from "@/api/queries";
import { Button, Kbd, Menu, MenuContent, MenuLabel, MenuSeparator, MenuTrigger, Switch, toast } from "@/kit";
import { SHORTCUTS } from "@/lib/shortcuts";
import { useOsDark } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { useShell } from "@/shell/state";

import { GeneratorSettings } from "./GeneratorSettings";
import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection, useSearchQuery } from "./SettingsLayout";
import { CLIPBOARD_WAITS, describeWait, IDLE_LOCK_WAITS, type Wait, waitsWith } from "./waits";

const CONFIG_ERROR = "keefast's settings file could not be read or saved, so recent Vault files may be forgotten.";
const FAVICONS_NOTE =
  "keefast then asks each Entry's website for its icon, so those websites see your IP address. Nothing goes through anyone else, and the icons are kept in memory only, never on disk.";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

/** Shows a change at once, and keeps it. */
function useChange() {
  const client = useQueryClient();
  return useCallback(
    (patch: Partial<AppSettings>, keep: () => Promise<unknown>) => {
      client.setQueryData<AppSettings>(keys.settings, (settings) => settings && { ...settings, ...patch });
      keep().catch((error) => {
        fail(error);
        void client.invalidateQueries({ queryKey: keys.settings });
      });
    },
    [client],
  );
}

export function GeneralSettings({ section }: { section?: string }) {
  const settings = useSettings().data;
  const snapshot = useWorkspace().data;
  const change = useChange();
  const { actions } = useShell();

  const sections = useMemo<SettingsSection[]>(() => {
    if (!settings) return [];
    const browser = settings.browser;
    const list: SettingsSection[] = [
      {
        id: "appearance",
        name: "Appearance",
        cards: [[{ id: "theme", search: "Theme System Dark Light appearance colour color mode System follows your desktop's light or dark mode", render: () => <ThemeBlock theme={settings.theme} onChange={(theme) => change({ theme }, () => api.setTheme(theme))} /> }]],
      },
      {
        id: "locking",
        name: "Locking",
        desc: "When unlocked Vaults lock themselves.",
        cards: [[
          {
            id: "idle",
            search: "Lock when idle Lock every Vault after this long without input timeout auto-lock inactivity",
            render: () => (
              <SettingRow label="Lock when idle" desc="Lock every Vault after this long without input">
                <WaitPicker label="Lock after" offered={IDLE_LOCK_WAITS} value={settings.idleLockSeconds} onChange={(seconds) => change({ idleLockSeconds: seconds }, () => api.setIdleLockAfter(seconds))} />
              </SettingRow>
            ),
          },
          {
            id: "osLock",
            search: "Lock with the screen Lock every Vault when the screen locks or the computer sleeps sleep suspend",
            render: () => (
              <SettingRow label="Lock with the screen" desc="Lock every Vault when the screen locks or the computer sleeps">
                <Switch label="Lock with the screen" checked={settings.lockOnOsLock} onCheckedChange={(enabled) => change({ lockOnOsLock: enabled }, () => api.setLockOnOsLock(enabled))} />
              </SettingRow>
            ),
          },
        ]],
      },
      {
        id: "clipboard",
        name: "Clipboard",
        cards: [[{
          id: "clear",
          search: "Clear clipboard Clear copied usernames and passwords after this long copy paste",
          render: () => (
            <SettingRow label="Clear clipboard" desc="Clear copied usernames and passwords after this long">
              <WaitPicker label="Clear after" offered={CLIPBOARD_WAITS} value={settings.clipboardClearSeconds} onChange={(seconds) => change({ clipboardClearSeconds: seconds }, () => api.setClipboardClearAfter(seconds))} />
            </SettingRow>
          ),
        }]],
      },
      {
        id: "saving",
        name: "Saving",
        cards: [[{
          id: "autosave",
          search: "Autosave Save Vaults automatically after every change save write disk",
          render: () => (
            <SettingRow label="Autosave" desc="Save Vaults automatically after every change">
              <Switch label="Autosave" checked={settings.autosave} onCheckedChange={(enabled) => change({ autosave: enabled }, () => api.setAutosave(enabled))} />
            </SettingRow>
          ),
        }]],
      },
      {
        id: "generator",
        name: "Password generator",
        desc: "keefast and the Companion extension generate passwords with these settings.",
        cards: [[{ id: "generator", search: "Password generator length characters passphrase words separator symbols look-alikes defaults", render: () => <GeneratorSettings options={settings.generator} /> }]],
      },
      {
        id: "browser",
        name: "Browser",
        desc: "Fill Entries in your browser with the Companion extension.",
        cards: [
          [
            {
              id: "integration",
              search: "Browser integration Let browser extensions ask keefast for Entries to fill (KeePassXC-Browser protocol) extension chrome brave edge vivaldi chromium companion keepassxc",
              render: () => (
                <SettingRow label="Browser integration" desc="Let browser extensions ask keefast for Entries to fill (KeePassXC-Browser protocol)">
                  <Switch label="Browser integration" checked={browser.enabled} onCheckedChange={(enabled) => change({ browser: { ...browser, enabled } }, () => api.setBrowserIntegration(enabled))} />
                </SettingRow>
              ),
            },
            ...(browser.enabled ? [{ id: "status", search: `Browser integration status ${browser.message ?? ""}`, render: () => <BrowserStatusNote status={browser} /> }] : []),
          ],
          ...(browser.enabled
            ? [[
                { id: "pairings", search: "Pairings paired browsers Vault Default access policy", render: () => <SettingRow className="min-h-12" label="Pairings" desc="Each Vault keeps its own Pairings and Default access policy." /> },
                ...(snapshot?.vaults ?? [])
                  .filter((vault) => vault.state === "unlocked")
                  .map((vault) => ({
                    id: `pairings-${vault.id}`,
                    search: `Pairings paired browsers ${vault.name}`,
                    render: () => <VaultPairings vault={vault} onManage={() => actions.openVaultSettings(vault.id, "pairings")} />,
                  })),
              ]]
            : []),
        ],
      },
    ];
    if (snapshot?.dropboxAvailable) {
      list.push({
        id: "dropbox",
        name: "Dropbox",
        desc: "Open and sync Vault files kept in Dropbox.",
        cards: [[{ id: "account", search: "Dropbox account connect disconnect sync cloud", render: () => <DropboxAccount /> }]],
      });
    }
    list.push(
      {
        id: "plugins",
        name: "Plugins",
        desc: "Teach keefast kinds of data kept on Entries, such as payment cards.",
        cards: [[{ id: "plugins", search: "Plugins install enable disable remove card kfplugin", render: () => <PluginsLink onManage={() => actions.openPlugins()} /> }]],
      },
      {
        id: "network",
        name: "Network",
        cards: [[
          {
            id: "favicons",
            search: "Website icons Download website icons for Entries favicon network privacy ip",
            render: () => (
              <SettingRow label="Website icons" desc="Download website icons for Entries">
                <Switch label="Website icons" checked={settings.downloadFavicons} onCheckedChange={(enabled) => change({ downloadFavicons: enabled }, () => api.setDownloadFavicons(enabled))} />
              </SettingRow>
            ),
          },
          {
            id: "faviconsNote",
            search: `privacy ip address ${FAVICONS_NOTE}`,
            render: () => (
              <div className="set-note">
                <Shield />
                <span>
                  <Hl>{FAVICONS_NOTE}</Hl>
                </span>
              </div>
            ),
          },
        ]],
      },
      {
        id: "shortcuts",
        name: "Keyboard shortcuts",
        desc: "Read-only for now.",
        cards: [[{ id: "keys", search: KEYS.flatMap(([, rows]) => rows.map(([name]) => name)).join(" · "), render: () => <KeysBlock /> }]],
      },
    );
    return list;
  }, [settings, snapshot, change, actions]);

  return (
    <SettingsLayout
      crumbs={
        <>
          Settings <CrumbSep /> <b>General</b>
        </>
      }
      searchLabel="Search settings"
      title="General Settings"
      sub="Kept on this machine and shared by every Vault. Each Vault's own settings live with the Vault."
      banner={
        settings?.configError ? (
          <div className="banner" role="alert" title={settings.configError}>
            <CircleAlert /> {CONFIG_ERROR}
          </div>
        ) : null
      }
      sections={sections}
      tip="Changes apply immediately."
      emptyLabel="settings"
      section={section}
    />
  );
}

// ---------------------------------------------------------------------------------------------
// Appearance

const THEMES: readonly { value: Theme; name: string }[] = [
  { value: "system", name: "System" },
  { value: "dark", name: "Dark" },
  { value: "light", name: "Light" },
];

function ThemeBlock({ theme, onChange }: { theme: Theme; onChange: (theme: Theme) => void }) {
  const osDark = useOsDark();
  return (
    <div className="set-block">
      <div className="hstack mb-3">
        <span className="font-medium">
          <Hl>Theme</Hl>
        </span>
        <span className="spacer" />
        <span className="hint">
          <Hl>System follows your desktop's light or dark mode</Hl>
        </span>
      </div>
      <div className="themes" role="radiogroup" aria-label="Theme">
        {THEMES.map((option) => {
          const on = option.value === theme;
          return (
            <button key={option.value} type="button" role="radio" aria-checked={on} className={cn("theme-tile", on && "is-on")} onClick={() => onChange(option.value)}>
              <div className="pv">
                {option.value === "light" ? <Mini kind="pv-light" /> : <Mini kind="pv-dark" />}
                {option.value === "system" && <Mini kind="pv-light split" />}
              </div>
              <div className="cap">
                <span className={cn("radio", on && "is-on")} />
                {option.name}
                {option.value === "system" && <span className="subtle">{osDark ? "Dark now" : "Light now"}</span>}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** A tiny window in a theme's colours. */
function Mini({ kind }: { kind: string }) {
  return (
    <div className={`mini ${kind}`} aria-hidden>
      <div className="ms">
        <i className="sel" style={{ width: "70%" }} />
        <i style={{ width: "55%" }} />
        <i style={{ width: "80%" }} />
        <i style={{ width: "45%" }} />
      </div>
      <div className="mm">
        <div className="r"><b /><i /></div>
        <div className="r sel"><b /><i /></div>
        <div className="r"><b /><i style={{ flex: 0.6 }} /></div>
        <div className="r"><b /><i style={{ flex: 0.8 }} /></div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Locking and clipboard

function WaitPicker({ label, offered, value, onChange }: { label: string; offered: readonly Wait[]; value: Wait; onChange: (wait: Wait) => void }) {
  const waits = waitsWith(offered, value);
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="dd" aria-label={`${label}: ${describeWait(value)}`}>
          <span>{describeWait(value)}</span>
          <ChevronsUpDown />
        </button>
      </MenuTrigger>
      <MenuContent align="end" className="dd-menu min-w-[160px]">
        <MenuLabel>{label}</MenuLabel>
        <MenuPrimitive.RadioGroup value={String(value)} onValueChange={(next) => onChange(next === "null" ? null : Number(next))}>
          {waits.map((wait) => (
            <WaitItem key={String(wait)} wait={wait} chosen={wait === value} />
          ))}
        </MenuPrimitive.RadioGroup>
      </MenuContent>
    </Menu>
  );
}

function WaitItem({ wait, chosen }: { wait: Wait; chosen: boolean }) {
  return (
    <>
      {wait === null && <MenuSeparator />}
      <MenuPrimitive.RadioItem value={String(wait)} className={cn("menu-item outline-none data-[highlighted]:bg-bg-active", chosen && "is-active")}>
        {describeWait(wait)}
        <MenuPrimitive.ItemIndicator className="chk">
          <CheckIcon />
        </MenuPrimitive.ItemIndicator>
      </MenuPrimitive.RadioItem>
    </>
  );
}

// ---------------------------------------------------------------------------------------------
// Browser

const BROWSER_COLORS: Record<string, string> = {
  Chrome: "#4285f4",
  Chromium: "#5f6368",
  Brave: "#fb542b",
  Edge: "#0c8ce9",
  Vivaldi: "#ef3939",
};

/** Where browser integration stands, from `get_settings`: set up for some browsers, none
 * found, or what failed (with a way to try again). */
function BrowserStatusNote({ status }: { status: AppSettings["browser"] }) {
  const [retrying, setRetrying] = useState(false);
  const retry = () => {
    setRetrying(true);
    // Turning it on again registers the native host again and serves the browsers.
    api.setBrowserIntegration(true).catch(fail).finally(() => setRetrying(false));
  };
  if (retrying) {
    return (
      <div className="bstatus">
        <LoaderCircle className="ico-sm set-spin" /> Setting up browsers…
      </div>
    );
  }
  if (!status.message) return null;
  if (status.error) {
    return (
      <div className="bstatus danger" role="alert">
        <CircleAlert /> {status.message}
        <Button variant="ghost" size="sm" onClick={retry}>
          <RotateCw /> Try again
        </Button>
      </div>
    );
  }
  if (status.browsers.length === 0) {
    return (
      <div className="bstatus warn">
        <TriangleAlert /> {status.message}
        <Button variant="ghost" size="sm" onClick={retry}>
          Check again
        </Button>
      </div>
    );
  }
  return (
    <div className="bstatus">
      <span className="status-dot" />
      Set up for
      {status.browsers.map((name) => (
        <span key={name} className="browser-chip">
          <span className="bi" style={{ background: BROWSER_COLORS[name] ?? "var(--fg-2)" }}>
            {name[0]}
          </span>
          {name}
        </span>
      ))}
      <Button variant="ghost" size="sm" onClick={retry}>
        <RotateCw /> Check again
      </Button>
    </div>
  );
}

/** One unlocked Vault's Pairings, and the way to its Vault settings. */
function VaultPairings({ vault, onManage }: { vault: VaultSummary; onManage: () => void }) {
  const count = useVaultSettings(vault.id).data?.pairings.length;
  return (
    <div className="pair-row">
      {vault.location.provider === "dropbox" ? <Cloud className="ico-sm" /> : <LockOpen className="ico-sm" />}
      <span className="font-medium">
        <Hl>{vault.name}</Hl>
      </span>
      <span className="muted">{count === undefined ? "" : count === 0 ? "No Pairings" : count === 1 ? "1 paired browser" : `${count} paired browsers`}</span>
      <Button variant="ghost" size="sm" onClick={onManage}>
        Manage <ArrowUpRight />
      </Button>
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Plugins

function PluginsLink({ onManage }: { onManage: () => void }) {
  const plugins = usePlugins().data?.plugins;
  const names = plugins?.filter((plugin) => plugin.enabled).map((plugin) => plugin.name) ?? [];
  const desc = !plugins ? "" : names.length === 0 ? "None enabled" : `Enabled: ${names.join(", ")}`;
  return (
    <SettingRow label="Plugins" desc={desc}>
      <Button size="sm" onClick={onManage}>
        Manage <ArrowUpRight />
      </Button>
    </SettingRow>
  );
}

// ---------------------------------------------------------------------------------------------
// Dropbox

function DropboxAccount() {
  const client = useQueryClient();
  const status = useQuery({ queryKey: keys.dropbox, queryFn: api.dropboxStatus });
  const { actions } = useShell();
  const [connecting, setConnecting] = useState(false);
  const refresh = () => client.invalidateQueries({ queryKey: keys.dropbox });
  const account = status.data?.account;

  const connect = () => {
    setConnecting(true);
    api
      .dropboxConnect()
      .then((connected) => toast(`Connected to Dropbox as ${connected.name}`, { icon: "check" }))
      .catch((error) => {
        if ((error as { kind?: string }).kind !== "cancelled") fail(error);
      })
      .finally(() => {
        setConnecting(false);
        void refresh();
      });
  };
  const disconnect = () =>
    actions.confirm({
      title: "Disconnect Dropbox?",
      description: "keefast forgets its access to your Dropbox. Vaults in Dropbox open again once you connect it again.",
      confirmLabel: "Disconnect",
      danger: true,
      onConfirm: () => api.dropboxDisconnect().then(refresh, (error) => void fail(error)),
    });

  if (connecting || status.data?.connecting) {
    return (
      <SettingRow label="Dropbox account" desc="Waiting for you to allow keefast in your browser…">
        <Button size="sm" variant="ghost" onClick={() => void api.dropboxReopenConnectPage().catch(fail)}>
          Open the page again
        </Button>
        <Button size="sm" onClick={() => void api.dropboxCancelConnect().catch(fail)}>
          Cancel
        </Button>
      </SettingRow>
    );
  }
  if (status.error) {
    return (
      <SettingRow label="Dropbox account" desc={<span className="text-danger">{errorMessage(status.error)}</span>}>
        <Button size="sm" onClick={() => void refresh()}>
          Try again
        </Button>
      </SettingRow>
    );
  }
  if (!status.data) return <SettingRow label="Dropbox account" desc="Checking…" />;
  return account ? (
    <SettingRow label="Dropbox account" desc={`Connected as ${account.name} · ${account.email}`}>
      <Button size="sm" onClick={disconnect}>
        Disconnect
      </Button>
    </SettingRow>
  ) : (
    <SettingRow label="Dropbox account" desc="Not connected. Connect it to open Vault files kept in Dropbox.">
      <Button size="sm" onClick={connect}>
        Connect…
      </Button>
    </SettingRow>
  );
}

// ---------------------------------------------------------------------------------------------
// Keyboard shortcuts

/** The shortcuts DESIGN.md §5 gives, by group: a name, and its keys (alternatives, or a pair). */
const KEYS: readonly [string, readonly [string, readonly string[], ("or" | "pair")?][]][] = [
  ["General", [
    ["Command palette", SHORTCUTS.palette.slice(0, 1)],
    ["Search", SHORTCUTS.search, "or"],
    ["Open Vault", SHORTCUTS.openVault],
    ["New Vault", SHORTCUTS.newVault],
    ["Save all Vaults", SHORTCUTS.save],
    ["Lock all Vaults", SHORTCUTS.lockAll],
    ["Settings", SHORTCUTS.settings],
    ["Close or go back", SHORTCUTS.close],
  ]],
  ["Entries", [
    ["Next / previous Entry", [...SHORTCUTS.nextEntry, ...SHORTCUTS.previousEntry], "pair"],
    ["First / last Entry", [...SHORTCUTS.firstEntry, ...SHORTCUTS.lastEntry], "pair"],
    ["Edit Entry", SHORTCUTS.edit.slice(0, 1)],
    ["New Entry", SHORTCUTS.newEntry],
    ["Copy username", SHORTCUTS.copyUsername],
    ["Copy password", SHORTCUTS.copyPassword],
    ["Copy TOTP code", SHORTCUTS.copyTotp],
    ["Entry history", SHORTCUTS.history],
  ]],
];

function KeysBlock() {
  const query = useSearchQuery().toLowerCase();
  // A search for the section itself shows every shortcut; another, those it matches.
  const whole = !query || "keyboard shortcuts read-only for now.".includes(query);
  const columns = KEYS.map(([group, rows]) => [group, rows.filter(([name]) => whole || name.toLowerCase().includes(query))] as const).filter(([, rows]) => rows.length > 0);
  return (
    <div className={cn("keys", columns.length === 1 && "one")}>
      {columns.map(([group, rows]) => (
        <div key={group} className="kcol">
          <div className="kgroup">{group}</div>
          {rows.map(([name, combos, join]) => (
            <div key={name} className="krow">
              <span>
                <Hl>{name}</Hl>
              </span>
              <span className="kk">
                {combos.map((combo, index) => (
                  <span key={combo} className="inline-flex items-center gap-1">
                    {index > 0 && join === "or" && <span className="or">or</span>}
                    <Kbd combo={combo} />
                  </span>
                ))}
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
