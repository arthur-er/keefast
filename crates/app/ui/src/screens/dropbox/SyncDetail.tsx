// A Dropbox Vault's Sync status, told in full: the head of the toolbar's status popover when the
// Vault shown is in Dropbox (the prototype's `dropbox.html?variant=A&state=sync`). The word and
// dot themselves are the shell's (`shell/vaultStatus.ts`), beside the Vault in the sidebar and in
// the toolbar.

import { CircleAlert, CloudOff, RefreshCw } from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { SyncState, VaultSummary } from "@/bindings";
import { keys } from "@/api/queries";
import { Button, toast } from "@/kit";
import { timeAgo } from "@/lib/format";
import { useNow } from "@/lib/now";
import { cn } from "@/lib/utils";

import "./dropbox.css";

const SYNC: Record<SyncState, { word: string; body: string }> = {
  synced: { word: "Synced", body: "The Vault file in Dropbox has everything saved here." },
  syncing: { word: "Syncing…", body: "Checking the Vault file in Dropbox for changes, or uploading the changes saved here." },
  offline: { word: "Offline", body: "Dropbox can't be reached. Saves are kept on this machine and upload when Dropbox is back." },
  error: { word: "Sync error", body: "The last sync with the Vault file in Dropbox failed." },
};

export function SyncDetail({ vault }: { vault: VaultSummary }) {
  const account = useQuery({ queryKey: keys.dropbox, queryFn: api.dropboxStatus }).data?.account;
  const now = useNow();
  const sync = vault.syncStatus;
  if (!sync) return null;
  const { word, body } = SYNC[sync];
  return (
    <div className="dbx-sync">
      <div className={cn("ttl", sync)}>
        {sync === "syncing" ? (
          <RefreshCw className="dbx-spin" />
        ) : sync === "offline" ? (
          <CloudOff />
        ) : sync === "error" ? (
          <CircleAlert />
        ) : (
          <span className="status-dot" />
        )}
        {word}
      </div>
      <p>
        {body}
        {vault.lastSynced !== null && ` Last synced ${timeAgo(vault.lastSynced, now)}.`}
      </p>
      {(sync === "error" || sync === "offline") && (
        <div className="hstack mt-2.5">
          <Button
            size="sm"
            onClick={() => void api.syncNow().catch((failure) => toast(errorMessage(failure), { icon: "error" }))}
          >
            <RefreshCw /> Retry
          </Button>
          <span className="hint">keefast Merges both versions.</span>
        </div>
      )}
      <div className="loc">
        <span className="k">Vault file</span>
        <span className="v mono truncate" title={vault.location.description}>
          {vault.location.description}
        </span>
        {account && (
          <>
            <span className="k">Account</span>
            <span className="v truncate">{account.email || account.name}</span>
          </>
        )}
      </div>
    </div>
  );
}
