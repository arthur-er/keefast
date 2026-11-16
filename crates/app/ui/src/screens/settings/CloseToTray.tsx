// General Settings' "Close to tray" row (ADR 0013). Where the desktop has no Tray, it says that
// closing will minimize instead.

import type { ReactNode } from "react";

import { Switch } from "@/kit";

import { SettingRow } from "./SettingsLayout";

export const CLOSE_TO_TRAY_DESC = "Keep keefast running in the system tray when you close the window, so the browser extension keeps working";
export const NO_TRAY_DESC = "No system tray found; closing will minimize";

/** What a settings search finds the row by. */
export const CLOSE_TO_TRAY_SEARCH = `Close to tray ${CLOSE_TO_TRAY_DESC} ${NO_TRAY_DESC} system tray notification area background hide window minimize quit`;

export function CloseToTrayRow({ enabled, trayAvailable, onChange }: { enabled: boolean; trayAvailable: boolean; onChange: (enabled: boolean) => void }) {
  const desc: ReactNode = trayAvailable ? CLOSE_TO_TRAY_DESC : NO_TRAY_DESC;
  return (
    <SettingRow label="Close to tray" desc={desc}>
      <Switch label="Close to tray" checked={enabled} onCheckedChange={onChange} />
    </SettingRow>
  );
}
