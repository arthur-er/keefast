// General Settings C: the app config's machine-local preferences, on one long page with an
// "On this page" index and `/` search. Each change is kept as soon as it is made.

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ArrowUpRight, Check as CheckIcon, ChevronsUpDown, CircleAlert, Cloud, LoaderCircle, LockOpen, RotateCcw, RotateCw, Shield, TriangleAlert,
} from "lucide-react";
import { DropdownMenu as MenuPrimitive } from "radix-ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AppSettings, Theme, VaultSummary } from "@/bindings";
import { keys, usePlugins, useSettings, useVaultSettings, useWorkspace } from "@/api/queries";
import { Button, Kbd, Menu, MenuContent, MenuLabel, MenuSeparator, MenuTrigger, Switch, toast } from "@/kit";
import { comboFromEvent, comboLabel, DEFAULT_SHORTCUTS, FIXED_SHORTCUTS, holderOf, type ShortcutName, useShortcuts } from "@/lib/shortcuts";
import { useOsDark } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { disconnectConfirmation, dropboxVaults } from "@/screens/dropbox/account";
import { useShell } from "@/shell/state";

import { CLOSE_TO_TRAY_SEARCH, CloseToTrayRow } from "./CloseToTray";
import { GeneratorSettings } from "./GeneratorSettings";
import { lastUsed, usedText } from "./pairings";
import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection, useSearchQuery } from "./SettingsLayout";
import { START_ON_BOOT_SEARCH, StartOnBootRow } from "./StartOnBoot";
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
        id: "window",
        name: "Window & startup",
        cards: [[
          {
            id: "closeToTray",
            search: CLOSE_TO_TRAY_SEARCH,
            render: () => (
              <CloseToTrayRow
                enabled={settings.closeToTray}
                trayAvailable={settings.trayAvailable}
                // Turned on, keefast looks for a Tray again; the settings it sends next say what it found.
                onChange={(enabled) => change({ closeToTray: enabled }, () => api.setCloseToTray(enabled))}
              />
            ),
          },
          { id: "startOnBoot", search: START_ON_BOOT_SEARCH, render: () => <StartOnBootRow /> },
        ]],
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
        desc: KEYS_DESC,
        cards: [[{ id: "keys", search: KEYS.flatMap(([, rows]) => rows.map(([title]) => title)).join(" · "), render: () => <KeysBlock /> }]],
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

/** One unlocked Vault's Pairings, when one was last used, and the way to its Vault settings. */
function VaultPairings({ vault, onManage }: { vault: VaultSummary; onManage: () => void }) {
  const pairings = useVaultSettings(vault.id).data?.pairings;
  const count = pairings?.length;
  const used = pairings && pairings.length > 0 ? ` · ${usedText(lastUsed(pairings)).text.toLowerCase()}` : "";
  return (
    <div className="pair-row">
      {vault.location.provider === "dropbox" ? <Cloud className="ico-sm" /> : <LockOpen className="ico-sm" />}
      <span className="font-medium">
        <Hl>{vault.name}</Hl>
      </span>
      <span className="muted">{count === undefined ? "" : count === 0 ? "No Pairings" : count === 1 ? "1 paired browser" : `${count} paired browsers`}{used}</span>
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
  const open = dropboxVaults(useWorkspace().data?.vaults ?? []);
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
      ...disconnectConfirmation(open),
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

/** A row of the list: one shortcut by its name in `shortcuts.json`, or a fixed pair. */
type KeyRow = readonly [title: string, shortcut: ShortcutName | readonly [ShortcutName, ShortcutName]];

/** The shortcuts DESIGN.md §5 gives, by group. */
const KEYS: readonly [string, readonly KeyRow[]][] = [
  ["General", [
    ["Command palette", "palette"],
    ["Search", "search"],
    ["Open Vault", "openVault"],
    ["New Vault", "newVault"],
    ["Save all Vaults", "save"],
    ["Lock all Vaults", "lockAll"],
    ["Settings", "settings"],
    ["Quit keefast", "quit"],
    ["Close or go back", "close"],
  ]],
  ["Entries", [
    ["Next / previous Entry", ["nextEntry", "previousEntry"]],
    ["First Entry", "firstEntry"],
    ["Last Entry", "lastEntry"],
    ["Edit Entry", "edit"],
    ["New Entry", "newEntry"],
    ["Copy username", "copyUsername"],
    ["Copy password", "copyPassword"],
    ["Copy TOTP code", "copyTotp"],
    ["Entry history", "history"],
    ["Generate a password", "generatePassword"],
    ["Delete Entry", "deleteEntry"],
  ]],
];

const KEYS_DESC = "Click a shortcut's keys to give it others.";

function KeysBlock() {
  const query = useSearchQuery().toLowerCase();
  const shortcuts = useShortcuts();
  const changed = useSettings().data?.shortcuts ?? {};
  const [recording, setRecording] = useState<ShortcutName | null>(null);
  // A search for the section itself shows every shortcut; another, those it matches.
  const whole = !query || `keyboard shortcuts ${KEYS_DESC}`.toLowerCase().includes(query);
  const columns = KEYS.map(([group, rows]) => [group, rows.filter(([title]) => whole || title.toLowerCase().includes(query))] as const).filter(([, rows]) => rows.length > 0);
  /** Every shortcut but `name`, by its title, to say which one has a key. */
  const others = (name: ShortcutName) =>
    KEYS.flatMap(([, rows]) => rows).flatMap(([title, shortcut]) =>
      (typeof shortcut === "string" ? [shortcut] : shortcut).filter((other) => other !== name).map((other) => ({ title, keys: shortcuts[other] })),
    );
  return (
    <div className={cn("keys", columns.length === 1 && "one")}>
      {columns.map(([group, rows]) => (
        <div key={group} className="kcol">
          <div className="kgroup">{group}</div>
          {rows.map(([title, shortcut]) =>
            typeof shortcut === "string" ? (
              <ShortcutRow
                key={title}
                title={title}
                name={shortcut}
                combos={shortcuts[shortcut]}
                changed={shortcut in changed}
                recording={recording === shortcut}
                onRecord={(on) => setRecording(on ? shortcut : null)}
                others={() => others(shortcut)}
              />
            ) : (
              <div key={title} className="krow" title="Arrow keys move through every list: they can't be changed.">
                <span>
                  <Hl>{title}</Hl>
                </span>
                <span className="kk">
                  {shortcut.flatMap((name) => shortcuts[name]).map((combo) => (
                    <Kbd key={combo} combo={combo} />
                  ))}
                </span>
              </div>
            ),
          )}
        </div>
      ))}
    </div>
  );
}

/** A shortcut and its keys. Unless it is fixed, clicking the keys records new ones: the next
 * key pressed with its modifiers, refused when another shortcut has it (Escape gives up). A
 * shortcut the user changed can go back to its default keys. */
function ShortcutRow({
  title,
  name,
  combos,
  changed,
  recording,
  onRecord,
  others,
}: {
  title: string;
  name: ShortcutName;
  combos: readonly string[];
  changed: boolean;
  recording: boolean;
  onRecord: (on: boolean) => void;
  others: () => { title: string; keys: readonly string[] }[];
}) {
  const client = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const fixed = FIXED_SHORTCUTS.includes(name);
  const defaults = DEFAULT_SHORTCUTS[name];

  const save = async (combo: string | null) => {
    try {
      await api.setShortcut(name, combo);
      await client.invalidateQueries({ queryKey: keys.settings });
      setError(null);
      onRecord(false);
    } catch (failure) {
      setError(errorMessage(failure));
    }
  };

  useEffect(() => {
    if (!recording) return;
    setError(null);
    // Ahead of every other key binding: the key pressed is the one to record.
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape" && !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey) {
        setError(null);
        onRecord(false);
        return;
      }
      const combo = comboFromEvent(event);
      if (!combo) return;
      const holder = holderOf(combo, others());
      if (holder) setError(`${comboLabel(combo).join(" ")} is taken by ${holder.title}.`);
      else void save(combo);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // `others` and `save` read the shortcuts as they are when the key is pressed.
  }, [recording]);

  const caps = combos.map((combo, index) => (
    <span key={combo} className="inline-flex items-center gap-1">
      {index > 0 && <span className="or">or</span>}
      <Kbd combo={combo} />
    </span>
  ));
  return (
    <>
      <div className={cn("krow", recording && "is-recording")}>
        <span>
          <Hl>{title}</Hl>
        </span>
        {changed && !recording && (
          <button
            type="button"
            className="kreset"
            title={`Back to ${defaults.join(" or ")}`}
            aria-label={`Give ${title} its default keys back`}
            onClick={() => void save(null)}
          >
            <RotateCcw />
          </button>
        )}
        {fixed ? (
          <span className="kk" title="Escape and the arrows keep their jobs in every list and dialog: they can't be changed.">
            {caps}
          </span>
        ) : (
          <button
            type="button"
            className="kk kedit"
            aria-label={recording ? `Press the new keys for ${title}, or Escape` : `Change the keys of ${title}`}
            title={recording ? undefined : "Change"}
            onClick={() => onRecord(!recording)}
          >
            {recording ? <span className="krec">Press the new keys…</span> : caps}
          </button>
        )}
      </div>
      {error && (
        <div className="kerr" role="alert">
          <CircleAlert /> {error}
        </div>
      )}
    </>
  );
}
