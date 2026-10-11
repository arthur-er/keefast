// Settings → Plugins (ADR 0009), laid out as Settings C: the Plugins installed, keefast's own
// first, each with its publisher, what the user agreed it may do, and a switch to enable or
// disable it; then installing one from a package file. Disabling or removing a Plugin leaves its
// data in the Vaults as ordinary custom fields.

import { useMemo } from "react";
import { BadgeCheck, CircleAlert, FileUp, ShieldAlert, Trash2, TriangleAlert } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { PluginList, PluginView } from "@/bindings";
import { keys, usePlugins } from "@/api/queries";
import { Badge, Button, Switch, toast, Tooltip } from "@/kit";
import { CAPABILITY_ICONS, installFromFile, PluginIcon } from "@/screens/plugins/InstallDialog";
import { capabilityLines, listNames } from "@/screens/plugins/capabilities";
import { useShell } from "@/shell/state";

import { CrumbSep, Hl, SettingRow, SettingsLayout, type SettingsSection } from "./SettingsLayout";

import "@/screens/plugins/plugins.css";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

const KEEPS_DATA = "Disabling or removing a Plugin keeps its data: its fields show as ordinary custom fields.";
const SANDBOX = "Plugins run in a sandbox with no network or file access, and can only do what you agreed to when installing them. keefast checks each Plugin's signature every time it starts.";

export function PluginSettings({ section }: { section?: string }) {
  const list = usePlugins().data;
  const client = useQueryClient();
  const { actions } = useShell();

  const sections = useMemo<SettingsSection[]>(() => {
    if (!list) return [];
    const setEnabled = (plugin: PluginView, enabled: boolean) => {
      client.setQueryData<PluginList>(keys.plugins, (current) => current && { ...current, plugins: current.plugins.map((p) => (p.id === plugin.id ? { ...p, enabled } : p)) });
      api
        .setPluginEnabled(plugin.id, enabled)
        .then(() => toast(<>{enabled ? "Enabled" : "Disabled"} <b>{plugin.name}</b></>, { icon: "check" }))
        .catch((error) => {
          fail(error);
          void client.invalidateQueries({ queryKey: keys.plugins });
        });
    };
    const remove = (plugin: PluginView) =>
      actions.confirm({
        title: `Remove “${plugin.name}”?`,
        description: `Its data stays in your Vaults: ${listNames(plugin.ownedFields)} show as ordinary custom fields. keefast also forgets its publisher's key.`,
        confirmLabel: "Remove",
        danger: true,
        onConfirm: () =>
          api.removePlugin(plugin.id).then(
            () => void toast(<>Removed <b>{plugin.name}</b></>, { icon: Trash2 }),
            (error) => void fail(error),
          ),
      });
    return [
      {
        id: "installed",
        name: "Installed",
        desc: KEEPS_DATA,
        count: list.plugins.length,
        cards: [
          list.plugins.map((plugin) => ({
            id: plugin.id,
            search: [plugin.name, plugin.id, plugin.publisher.kind === "keefast" ? "keefast" : `${plugin.publisher.name} unverified`, ...plugin.ownedFields].join(" "),
            render: () => <PluginRow plugin={plugin} onEnabled={(enabled) => setEnabled(plugin, enabled)} onRemove={() => remove(plugin)} />,
          })),
        ],
      },
      {
        id: "install",
        name: "Install",
        cards: [
          [
            {
              id: "file",
              search: "Install from a file kfplugin package choose",
              render: () => (
                <SettingRow label="Install from a file" desc="Choose a .kfplugin package. keefast shows what it asks for before installing anything.">
                  <Button size="sm" onClick={() => void installFromFile(actions)}>
                    <FileUp /> Choose file…
                  </Button>
                </SettingRow>
              ),
            },
            {
              id: "sandbox",
              search: `sandbox network signature ${SANDBOX}`,
              render: () => (
                <div className="set-note">
                  <ShieldAlert />
                  <span>
                    <Hl>{SANDBOX}</Hl>
                  </span>
                </div>
              ),
            },
          ],
        ],
      },
    ];
  }, [list, client, actions]);

  return (
    <SettingsLayout
      crumbs={
        <>
          Settings <CrumbSep /> <b>Plugins</b>
        </>
      }
      searchLabel="Search Plugins"
      title="Plugins"
      sub={
        <>
          Plugins teach keefast kinds of data kept on Entries, such as payment cards. They are installed on this machine, not in a Vault.
        </>
      }
      banner={
        list?.error ? (
          <div className="banner" role="alert" title={list.error}>
            <CircleAlert /> keefast couldn't read its installed Plugins, so only its own run: {list.error}
          </div>
        ) : null
      }
      sections={sections}
      tip="Changes apply immediately."
      emptyLabel="Plugins"
      section={section}
    />
  );
}

function PluginRow({ plugin, onEnabled, onRemove }: { plugin: PluginView; onEnabled: (enabled: boolean) => void; onRemove: () => void }) {
  const publisher = plugin.publisher;
  return (
    <div className="pl-row">
      <PluginIcon icon={plugin.icon} size={32} />
      <div className="txt">
        <div className="lbl">
          <Hl>{plugin.name}</Hl>
          {plugin.version && <span className="ver">{plugin.version}</span>}
          {publisher.kind === "keefast" ? (
            <Tooltip
              content={
                publisher.release
                  ? `Signed by keefast's release key, ${publisher.fingerprint}`
                  : `Signed by a key this build of keefast made for itself, ${publisher.fingerprint}, which it trusts for its own Plugins only`
              }
            >
              <Badge tone={publisher.release ? "accent" : "neutral"}>
                <BadgeCheck /> {publisher.release ? "keefast" : "keefast dev build"}
              </Badge>
            </Tooltip>
          ) : (
            <Tooltip content={`Key ${publisher.fingerprint}, trusted since this Plugin was installed`}>
              <Badge tone="warn">Unverified</Badge>
            </Tooltip>
          )}
        </div>
        <div className="desc">
          {publisher.kind === "keefast" ? (
            <>
              {plugin.bundled ? (publisher.release ? "Comes with keefast" : "Comes with this development build") : "From keefast"} · {publisher.release ? "keefast's key" : "the build's own key"}{" "}
              <span className="fp">{publisher.fingerprint}</span>
            </>
          ) : (
            <>
              <Hl>{publisher.name || "Unknown publisher"}</Hl> · key <span className="fp">{publisher.fingerprint}</span>
            </>
          )}
        </div>
        <ul className="pl-caps" aria-label="What it may do">
          {capabilityLines(plugin).map((line) => {
            const Icon = CAPABILITY_ICONS[line.kind];
            return (
              <li key={line.kind} title={line.desc}>
                <Icon />
                {line.short}
              </li>
            );
          })}
        </ul>
        {plugin.enabled && plugin.problem && (
          <div className="pl-problem" role="alert">
            <TriangleAlert /> It doesn't run: {plugin.problem}
          </div>
        )}
      </div>
      <div className="ctl">
        {!plugin.bundled && (
          <Button size="sm" variant="ghost" onClick={onRemove}>
            Remove
          </Button>
        )}
        <Switch label={plugin.enabled ? `Disable ${plugin.name}` : `Enable ${plugin.name}`} checked={plugin.enabled} onCheckedChange={onEnabled} />
      </div>
    </div>
  );
}
