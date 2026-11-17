// The settings pages, which replace the Vault browser's list and detail and keep the sidebar
// (`ShellState.page`): General Settings C, Vault settings C, Settings → Plugins and Settings →
// Agent hosts.

import type { Page } from "@/shell/state";

import { AgentHostSettings } from "./AgentHostSettings";
import { GeneralSettings } from "./GeneralSettings";
import { PluginSettings } from "./PluginSettings";
import { VaultSettingsPage } from "./VaultSettings";

import "./settings.css";

export function SettingsPage({ page }: { page: Exclude<Page, { kind: "browser" }> }) {
  return page.kind === "settings" ? (
    <GeneralSettings key="settings" section={page.section} />
  ) : page.kind === "plugins" ? (
    <PluginSettings key="plugins" section={page.section} />
  ) : page.kind === "agentHosts" ? (
    <AgentHostSettings key="agentHosts" section={page.section} />
  ) : (
    <VaultSettingsPage key={page.vault} vault={page.vault} section={page.section} />
  );
}
