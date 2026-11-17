// The update notice over the Vault browser (ADR 0014): asks once whether to check for updates,
// then says when one is found, downloads it when the user says so, and says if that failed.

import { useSyncExternalStore } from "react";
import { CircleAlert, Download, LoaderCircle, Sparkles, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AppSettings } from "@/bindings";
import { keys, useSettings, useUpdates } from "@/api/queries";
import { Button, toast } from "@/kit";

import { ASK_UPDATES, ASK_UPDATES_DESC, downloadingText, updateNotice } from "./updates";

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

// The versions whose notice the user closed, until keefast restarts: the notice comes and goes
// with the Vault browser, so they're kept outside it.
let dismissed: ReadonlySet<string> = new Set();
const listeners = new Set<() => void>();
function dismiss(version: string) {
  dismissed = new Set([...dismissed, version]);
  listeners.forEach((listener) => listener());
}
function useDismissed() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => dismissed,
  );
}

/** The user's answer to checking for updates automatically, shown at once and kept. */
export function useAnswerCheckForUpdates() {
  const client = useQueryClient();
  return (enabled: boolean) => {
    client.setQueryData<AppSettings>(keys.settings, (settings) => settings && { ...settings, checkForUpdates: enabled });
    api.setCheckForUpdates(enabled).catch((error) => {
      fail(error);
      void client.invalidateQueries({ queryKey: keys.settings });
    });
  };
}

/** Downloads and installs the update, then keefast restarts. */
export function installUpdate() {
  api.installUpdate().catch(fail);
}

export function openReleasePage() {
  api.openReleasePage().catch(fail);
}

export function UpdateNotice() {
  const updates = useUpdates().data;
  const checkForUpdates = useSettings().data?.checkForUpdates;
  const answer = useAnswerCheckForUpdates();
  const notice = updateNotice(updates, checkForUpdates, useDismissed());
  if (!notice) return null;

  switch (notice.kind) {
    case "ask":
      return (
        <div className="vb-merge" role="status">
          <Sparkles />
          <span className="truncate" title={ASK_UPDATES_DESC}>
            <b>{ASK_UPDATES}</b>
            <span className="subtle"> · </span>
            {ASK_UPDATES_DESC}
          </span>
          <span className="spacer" />
          <Button variant="ghost" size="sm" onClick={() => answer(false)}>
            Don't check
          </Button>
          <Button variant="secondary" size="sm" onClick={() => answer(true)}>
            Check daily
          </Button>
        </div>
      );
    case "available":
      return (
        <div className="vb-merge" role="status">
          <Sparkles />
          <span className="truncate">
            <b>keefast {notice.update.version} is available</b>
            {notice.installs && (
              <>
                <span className="subtle"> · </span>
                keefast restarts to update, and your Vaults lock
              </>
            )}
          </span>
          <span className="spacer" />
          <Button variant="ghost" size="sm" onClick={openReleasePage}>
            What's new
          </Button>
          {notice.installs ? (
            <Button variant="secondary" size="sm" onClick={installUpdate}>
              Update and restart
            </Button>
          ) : (
            <Button variant="secondary" size="sm" onClick={openReleasePage}>
              <Download /> Download
            </Button>
          )}
          <DismissButton version={notice.update.version} />
        </div>
      );
    case "downloading":
      return (
        <div className="vb-merge" role="status">
          <LoaderCircle className="vb-spin" />
          <span className="truncate">
            <b>{downloadingText(notice.update, notice.progress)}</b>
            <span className="subtle"> · </span>
            keefast restarts once it's installed
          </span>
        </div>
      );
    case "failed":
      return (
        <div className="vb-merge" role="alert">
          <CircleAlert className="text-danger" />
          <span className="truncate" title={notice.message}>
            <b>keefast {notice.update.version} wasn't installed</b>
            <span className="subtle"> · </span>
            {notice.message}
          </span>
          <span className="spacer" />
          <Button variant="secondary" size="sm" onClick={installUpdate}>
            Try again
          </Button>
          <DismissButton version={notice.update.version} />
        </div>
      );
  }
}

function DismissButton({ version }: { version: string }) {
  return (
    <button type="button" className="btn ghost sm icon" aria-label="Dismiss" onClick={() => dismiss(version)}>
      <X />
    </button>
  );
}
