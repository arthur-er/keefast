// The toolbar over the Entry list and detail: where the user is, the save status (a popover
// with each open Vault), and the command palette's button. Under it, the last Merge notice.

import { Fragment, useEffect, useState } from "react";
import { Command, GitMerge, Hash, LayoutGrid, RefreshCw, Save, TriangleAlert, X } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { Scope } from "@/bindings";
import { useGroups, useWorkspace } from "@/api/queries";
import { Button, colorVar, Popover, PopoverContent, PopoverTrigger, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { SyncDetail } from "@/screens/dropbox/SyncDetail";

import { COLOR_NAMES, dotClass, groupPath } from "./Sidebar";
import { useShell } from "./state";
import { showView, useView } from "./view";
import { toolbarStatus, vaultIcon, vaultStatus } from "./vaultStatus";

export function Toolbar() {
  const { actions } = useShell();
  return (
    <div className="toolbar">
      <Crumbs />
      <span className="spacer" />
      <StatusButton />
      <span className="vdivider mx-1 h-4 self-center" />
      <Button variant="ghost" size="sm" onClick={actions.togglePalette} aria-label="Commands" shortcut="Ctrl K">
        <Command />
      </Button>
    </div>
  );
}

function Crumbs() {
  const view = useView();
  const snapshot = useWorkspace().data;
  const scope = view?.scope;
  const vaultId = scope && (scope.kind === "vault" || scope.kind === "group") ? scope.vault : undefined;
  const groups = useGroups(vaultId).data;
  if (!scope) return null;
  if (scope.kind === "allVaults") {
    return (
      <div className="crumbs">
        <LayoutGrid />
        <b>All Entries</b>
      </div>
    );
  }
  if (scope.kind === "tag") {
    return (
      <div className="crumbs">
        <Hash />
        <span>Tags</span>
        <span className="sep">/</span>
        <b className="truncate">{scope.tag}</b>
      </div>
    );
  }
  if (scope.kind === "color") {
    return (
      <div className="crumbs">
        <span className="dot" style={{ background: colorVar(scope.color) }} />
        <span>Colours</span>
        <span className="sep">/</span>
        <b>{COLOR_NAMES[scope.color]}</b>
      </div>
    );
  }
  const vault = snapshot?.vaults.find((v) => v.id === scope.vault);
  if (!vault) return null;
  const Icon = vaultIcon(vault);
  const parts = [vault.name];
  if (scope.kind === "group" && groups) {
    const path = groupPath(groups, scope.group).slice(1);
    parts.push(...path.map((group) => (group.isRecycleBin ? "Recycle Bin" : group.name)));
  }
  return (
    <div className="crumbs min-w-0">
      <Icon />
      {parts.map((part, index) => (
        <Fragment key={index}>
          {index > 0 && <span className="sep">/</span>}
          {index === parts.length - 1 ? <b className="truncate">{part}</b> : <span className="truncate">{part}</span>}
        </Fragment>
      ))}
    </div>
  );
}

function StatusButton() {
  const snapshot = useWorkspace().data;
  const view = useView();
  const vaults = snapshot?.vaults ?? [];
  const scope: Scope | undefined = view?.scope;
  const shownId = scope && (scope.kind === "vault" || scope.kind === "group") ? scope.vault : null;
  const shown = vaults.find((vault) => vault.id === shownId);
  const status = toolbarStatus(vaults, shown);
  const open = vaults.filter((vault) => vault.state !== "locked");
  const anyDropbox = open.some((vault) => vault.location.provider === "dropbox");
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn("vb-status", status.tone === "failed" && "is-failed", status.tone === "warn" && "is-warn")}
          title="Save status · Ctrl S saves every Vault"
        >
          {status.tone === "failed" ? <TriangleAlert className="ico-sm" /> : <span className={cn("status-dot", dotClass(status.tone))} />}
          {status.text}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="vb-status-pop">
        {shown?.state === "unlocked" && shown.syncStatus && <SyncDetail vault={shown} />}
        {open.map((vault) => {
          const Icon = vaultIcon(vault);
          const own = vaultStatus(vault);
          return (
            <div key={vault.id} className="vb-status-row">
              <span className="tile">
                <Icon className="ico-sm" />
              </span>
              <div className="vstack min-w-0 flex-1 gap-0">
                <span className="name truncate">{vault.name}</span>
                <span className="file truncate">{vault.location.description}</span>
              </div>
              <span className={cn("hstack gap-1.5 text-sm", own.tone === "failed" ? "text-danger" : "text-fg-1")}>
                <span className={cn("status-dot", dotClass(own.tone))} />
                {own.short}
              </span>
            </div>
          );
        })}
        <div className="menu-sep" />
        <div className="hstack gap-1 p-1">
          {anyDropbox && (
            <Button variant="ghost" size="sm" onClick={() => void api.syncNow().catch(showError)}>
              <RefreshCw /> Sync now
            </Button>
          )}
          <span className="spacer" />
          <Button variant="secondary" size="sm" shortcut="Ctrl S" onClick={() => void api.saveAll().catch(showError)}>
            <Save /> Save all
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** The last Merge: what changed, and a way to the Vault it changed. */
export function MergeNotice() {
  const { state, actions } = useShell();
  const merge = state.merge;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  if (!merge) return null;
  const [lead, rest] = splitMessage(merge.message);
  return (
    <div className="vb-merge" role="status">
      <GitMerge />
      <span className="truncate">
        <b>{lead}</b>
        {rest && (
          <>
            <span className="subtle"> · </span>
            {rest}
          </>
        )}
      </span>
      <span className="spacer" />
      <span className="subtle text-xs">{mergedWhen(merge.at, now)}</span>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => {
          actions.showBrowser();
          showView({ scope: { kind: "vault", vault: merge.vault }, entry: null });
        }}
      >
        Show {merge.vaultName}
      </Button>
      <button type="button" className="btn ghost sm icon" aria-label="Dismiss" onClick={actions.dismissMerge}>
        <X />
      </button>
    </div>
  );
}

/** "Just now", "5 min ago", or the time of day. */
export function mergedWhen(at: number, now: number): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} min ago`;
  return new Date(at).toLocaleTimeString("en", { hour: "numeric", minute: "2-digit" });
}

/** "Merged changes made elsewhere into Work: 2 Entries added" as its lead and what changed. */
export function splitMessage(message: string): [string, string] {
  const at = message.indexOf(": ");
  return at < 0 ? [message, ""] : [message.slice(0, at), message.slice(at + 2)];
}

function showError(error: unknown) {
  toast(errorMessage(error), { icon: "error" });
}
