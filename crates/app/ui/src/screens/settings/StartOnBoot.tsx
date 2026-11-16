// General Settings' "Start on boot" row. It reads the OS registration every time rather than a
// setting, so an entry the user removed by hand shows as off.

import { useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import { keys, useStartOnBoot } from "@/api/queries";
import { Switch, toast } from "@/kit";

import { SettingRow } from "./SettingsLayout";

export const START_ON_BOOT_DESC = "Start keefast in the background when you log in";

/** What a settings search finds the row by. */
export const START_ON_BOOT_SEARCH = `Start on boot ${START_ON_BOOT_DESC} startup login sign in autostart launch automatically`;

export function StartOnBootRow() {
  const client = useQueryClient();
  const registered = useStartOnBoot();

  const change = (enabled: boolean) => {
    client.setQueryData(keys.startOnBoot, enabled);
    api
      .setStartOnBoot(enabled)
      .catch((error) => toast(errorMessage(error), { icon: "error" }))
      // Whatever happened, show what the OS has now.
      .finally(() => void client.invalidateQueries({ queryKey: keys.startOnBoot }));
  };

  const desc = registered.error ? <span className="text-danger">{errorMessage(registered.error)}</span> : START_ON_BOOT_DESC;
  return (
    <SettingRow label="Start on boot" desc={desc}>
      <Switch label="Start on boot" checked={registered.data ?? false} disabled={registered.data === undefined} onCheckedChange={change} />
    </SettingRow>
  );
}
