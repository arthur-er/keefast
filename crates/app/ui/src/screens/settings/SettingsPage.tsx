// The settings pages, which replace the Vault browser's list and detail and keep the sidebar
// (`ShellState.page`): General Settings C and Vault settings C.

import type { Page } from "@/shell/state";

import { GeneralSettings } from "./GeneralSettings";
import { VaultSettingsPage } from "./VaultSettings";

import "./settings.css";

export function SettingsPage({ page }: { page: Exclude<Page, { kind: "browser" }> }) {
  return page.kind === "settings" ? (
    <GeneralSettings key="settings" section={page.section} />
  ) : (
    <VaultSettingsPage key={page.vault} vault={page.vault} section={page.section} />
  );
}
