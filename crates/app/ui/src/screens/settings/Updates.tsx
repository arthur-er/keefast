// General Settings' Updates rows (ADR 0014): whether keefast checks once a day, and where
// updating stands, with checking now and installing what was found.

import { useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import { keys, useUpdates } from "@/api/queries";
import { Button, Switch, toast } from "@/kit";
import { installUpdate, openReleasePage, useAnswerCheckForUpdates } from "@/screens/updates/UpdateNotice";
import { CHECK_UPDATES_DESC, foundUpdate, updateStatusText } from "@/screens/updates/updates";

import { SettingRow } from "./SettingsLayout";

/** What a settings search finds the rows by. */
export const UPDATES_SEARCH = `Check for updates ${CHECK_UPDATES_DESC} automatically version new release upgrade install download`;

export function CheckForUpdatesRow({ enabled }: { enabled: boolean | null }) {
  const answer = useAnswerCheckForUpdates();
  return (
    <SettingRow label="Check for updates" desc={CHECK_UPDATES_DESC}>
      <Switch label="Check for updates" checked={enabled === true} onCheckedChange={answer} />
    </SettingRow>
  );
}

export function UpdateStatusRow() {
  const client = useQueryClient();
  const state = useUpdates().data;
  if (!state) return null;
  const status = state.status;
  const update = foundUpdate(status);
  const failed = status.kind === "failed";

  const checkNow = () =>
    api
      .checkForUpdatesNow()
      .then((checked) => client.setQueryData(keys.updates, checked))
      .catch((error) => toast(errorMessage(error), { icon: "error" }));

  let action;
  if (status.kind === "checking" || status.kind === "downloading") {
    action = null;
  } else if (update && state.installsUpdates) {
    action = (
      <Button variant="primary" size="sm" onClick={installUpdate}>
        {failed ? "Try again" : "Update and restart"}
      </Button>
    );
  } else if (update) {
    action = (
      <Button size="sm" onClick={openReleasePage}>
        Download
      </Button>
    );
  } else {
    action = (
      <Button size="sm" onClick={() => void checkNow()}>
        Check now
      </Button>
    );
  }

  return (
    <SettingRow label="Version" desc={failed ? <span className="text-danger">{updateStatusText(state)}</span> : updateStatusText(state)}>
      {update && (
        <Button variant="ghost" size="sm" onClick={openReleasePage}>
          What's new
        </Button>
      )}
      {action}
    </SettingRow>
  );
}
