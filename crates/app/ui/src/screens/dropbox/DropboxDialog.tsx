// Open from Dropbox A: a file browser dialog over Open a Vault B. It connects the Dropbox account
// in the browser when there is none (keefast never sees the Dropbox password), browses the whole
// Dropbox, and gives back the Vault file picked; Open a Vault then unlocks it. The account menu
// connects another account in place of this one, or disconnects, forgetting the account's tokens.

import { useCallback, useEffect, useRef, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";
import {
  ArrowUp, ChevronDown, ChevronRight, CircleAlert, CloudOff, Database, ExternalLink, File, FileLock2, Folder, FolderOpen,
  LoaderCircle, Lock, Repeat, RotateCw, ShieldCheck, Unplug, X,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { api, ApiError, errorMessage } from "@/api";
import type { DropboxAccountView, DropboxEntry, Location } from "@/bindings";
import { keys } from "@/api/queries";
import { Button, Kbd, Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, toast } from "@/kit";
import { isTyping } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

import { crumbs, isVaultFile, modifiedLabel, parentFolder, sizeLabel } from "./browse";
import { DropboxMark } from "./DropboxMark";
import "./dropbox.css";

type Phase =
  | { kind: "checking" }
  | { kind: "connect"; error: string | null }
  | { kind: "connecting" }
  | { kind: "connected"; account: DropboxAccountView };

interface Listing {
  folder: string;
  entries: DropboxEntry[];
  loading: boolean;
  error: string | null;
}

/** The folder last browsed, to start there next time the dialog opens. */
let lastFolder = "";

export function DropboxDialog({ onPick, onClose }: { onPick: (location: Location) => void; onClose: () => void }) {
  const client = useQueryClient();
  const [phase, setPhase] = useState<Phase>({ kind: "checking" });
  const [listing, setListing] = useState<Listing>({ folder: lastFolder, entries: [], loading: true, error: null });
  const [selected, setSelected] = useState<string | null>(null);
  const request = useRef(0);
  const connecting = useRef(false);
  const list = useRef<HTMLDivElement>(null);

  /** The account went away (disconnected elsewhere, or Dropbox revoked keefast): connect again. */
  const lost = useCallback(
    async (error: string | null) => {
      const status = await api.dropboxStatus().catch(() => null);
      void client.invalidateQueries({ queryKey: keys.dropbox });
      if (!status?.account) setPhase({ kind: "connect", error });
      return !status?.account;
    },
    [client],
  );

  const open = useCallback(
    async (folder: string) => {
      const id = ++request.current;
      setSelected(null);
      setListing((current) => ({ folder, entries: current.folder === folder ? current.entries : [], loading: true, error: null }));
      try {
        const entries = await api.dropboxListFolder(folder);
        if (id !== request.current) return;
        lastFolder = folder;
        setListing({ folder, entries, loading: false, error: null });
      } catch (failure) {
        if (id !== request.current) return;
        const message = errorMessage(failure);
        if (await lost(message)) return;
        // A folder gone since last time: next time, start from the top.
        if (folder === lastFolder) lastFolder = "";
        setListing({ folder, entries: [], loading: false, error: message });
      }
    },
    [lost],
  );

  const connected = useCallback(
    (account: DropboxAccountView) => {
      setPhase({ kind: "connected", account });
      void open(lastFolder);
    },
    [open],
  );

  useEffect(() => {
    let live = true;
    api
      .dropboxStatus()
      .then((status) => {
        if (!live) return;
        if (status.account) connected(status.account);
        else setPhase({ kind: "connect", error: status.available ? null : "This build of keefast has no Dropbox app key." });
      })
      .catch((failure) => live && setPhase({ kind: "connect", error: errorMessage(failure) }));
    return () => {
      live = false;
      // Closed while connecting: stop waiting for the browser, freeing its loopback port.
      if (connecting.current) void api.dropboxCancelConnect();
    };
    // Only when the dialog opens.
  }, []);

  /** Connects an account, or with `from`, another account in place of that one. */
  const connect = async (from?: DropboxAccountView) => {
    setPhase({ kind: "connecting" });
    connecting.current = true;
    try {
      const account = await (from ? api.dropboxConnectAnother() : api.dropboxConnect());
      connecting.current = false;
      void client.invalidateQueries({ queryKey: keys.dropbox });
      toast(`Connected as ${account.email || account.name}`, { icon: "check" });
      if (from) {
        // Another Dropbox: its folders aren't the last account's.
        lastFolder = "";
        setListing({ folder: "", entries: [], loading: true, error: null });
      }
      connected(account);
    } catch (failure) {
      connecting.current = false;
      // Stopping is the user's own doing, not an error.
      const stopped = failure instanceof ApiError && failure.kind === "cancelled";
      const error = stopped ? null : errorMessage(failure);
      if (from) {
        // The account from before is still connected: back to its files.
        setPhase({ kind: "connected", account: from });
        if (error) toast(error, { icon: "error" });
        return;
      }
      setPhase({ kind: "connect", error });
    }
  };

  const disconnect = async () => {
    try {
      await api.dropboxDisconnect();
      lastFolder = "";
      setListing({ folder: "", entries: [], loading: false, error: null });
      setSelected(null);
      setPhase({ kind: "connect", error: null });
      void client.invalidateQueries({ queryKey: keys.dropbox });
      toast("Dropbox disconnected", { icon: Unplug });
    } catch (failure) {
      toast(errorMessage(failure), { icon: "error" });
    }
  };

  const entries = listing.entries;
  const selectedEntry = entries.find((entry) => entry.path === selected) ?? null;
  const pick = (entry: DropboxEntry) => (entry.isFolder ? void open(entry.path) : onPick(entry.location));
  const up = () => {
    const parent = parentFolder(listing.folder);
    if (parent !== null) void open(parent);
  };

  // Keeps the selected row in view as the arrows move it.
  useEffect(() => {
    if (!selected) return;
    list.current?.querySelector<HTMLElement>(`[data-path="${CSS.escape(selected)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (isTyping(event.target)) return;
    if (phase.kind === "connect" && event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      void connect();
      return;
    }
    if (phase.kind !== "connected" || event.ctrlKey || event.altKey || event.metaKey) return;
    const index = entries.findIndex((entry) => entry.path === selected);
    switch (event.key) {
      case "Backspace":
        event.preventDefault();
        up();
        break;
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        if (!entries.length) return;
        const next = event.key === "ArrowDown" ? Math.min(entries.length - 1, index + 1) : Math.max(0, index < 0 ? 0 : index - 1);
        setSelected(entries[next]!.path);
        break;
      }
      case "Enter":
        if (event.target instanceof HTMLButtonElement && !event.target.dataset.row) return;
        if (selectedEntry) {
          event.preventDefault();
          pick(selectedEntry);
        }
        break;
    }
  };

  const isConnected = phase.kind === "connected";
  return (
    <DialogPrimitive.Root open onOpenChange={(open) => !open && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="scrim fixed">
          <DialogPrimitive.Content className="dbx-dialog outline-none" onKeyDown={onKeyDown} aria-describedby="dbx-desc">
            <div className="dbx-head">
              <DropboxMark size="md" />
              <div className="min-w-0 flex-1">
                <DialogPrimitive.Title className="t">Open from Dropbox</DialogPrimitive.Title>
                <DialogPrimitive.Description id="dbx-desc" className="d">
                  {isConnected ? "Choose the Vault file to open." : "Open a Vault file stored in your Dropbox."}
                </DialogPrimitive.Description>
              </div>
              {phase.kind === "connected" && (
                <AccountMenu
                  account={phase.account}
                  onUseAnother={() => void connect(phase.account)}
                  onDisconnect={() => void disconnect()}
                />
              )}
              <DialogPrimitive.Close className="btn sm ghost icon" aria-label="Cancel" title="Cancel (Esc)">
                <X />
              </DialogPrimitive.Close>
            </div>

            {phase.kind === "connected" ? (
              <>
                <div className="dbx-bar">
                  <button type="button" className="btn sm ghost icon" aria-label="Up" title="Up (Backspace)" disabled={!listing.folder} onClick={up}>
                    <ArrowUp />
                  </button>
                  <div className="vdivider my-2.5 mx-0.5" />
                  <nav className="dbx-crumbs" aria-label="Folder">
                    {crumbs(listing.folder).map((crumb, index) => (
                      <span key={crumb.path} className="contents">
                        {index > 0 && (
                          <span className="sep">
                            <ChevronRight />
                          </span>
                        )}
                        <button type="button" className={cn(crumb.path === listing.folder && "cur")} onClick={() => void open(crumb.path)}>
                          {index === 0 && <DropboxMark />}
                          {crumb.name}
                        </button>
                      </span>
                    ))}
                  </nav>
                  <span className="spacer" />
                  <button type="button" className="btn sm ghost icon" aria-label="Refresh" title="Refresh" onClick={() => void open(listing.folder)}>
                    <RotateCw />
                  </button>
                </div>
                <Files
                  listing={listing}
                  selected={selected}
                  listRef={list}
                  onSelect={setSelected}
                  onPick={pick}
                  onUp={up}
                  onRetry={() => void open(listing.folder)}
                />
              </>
            ) : (
              <div className="dbx-files bordered">
                {phase.kind === "checking" ? (
                  <div className="dbx-center">
                    <span className="ico">
                      <LoaderCircle className="dbx-spin" />
                    </span>
                    <div className="msg">Looking for your Dropbox account…</div>
                  </div>
                ) : phase.kind === "connect" ? (
                  <Connect error={phase.error} onConnect={() => void connect()} />
                ) : (
                  <Connecting />
                )}
              </div>
            )}

            <div className="dbx-foot">
              {isConnected ? (
                <>
                  <div className="sel">
                    {selectedEntry && !selectedEntry.isFolder ? (
                      <>
                        <FileLock2 className="ico-sm" />
                        <span className="mono truncate">Dropbox: {selectedEntry.path}</span>
                      </>
                    ) : (
                      <span>Select a Vault file</span>
                    )}
                  </div>
                  <span className="spacer" />
                  <DialogPrimitive.Close className="btn ghost">Cancel</DialogPrimitive.Close>
                  <Button
                    variant="primary"
                    disabled={!selectedEntry || selectedEntry.isFolder}
                    onClick={() => selectedEntry && pick(selectedEntry)}
                  >
                    Open <Kbd combo="Enter" />
                  </Button>
                </>
              ) : (
                <>
                  <div className="sel">
                    <Lock className="ico-sm" />
                    <span>Your Dropbox sign-in is kept in the OS secret store.</span>
                  </div>
                  <span className="spacer" />
                  <DialogPrimitive.Close className="btn ghost">
                    Cancel <Kbd combo="Escape" />
                  </DialogPrimitive.Close>
                </>
              )}
            </div>
          </DialogPrimitive.Content>
        </DialogPrimitive.Overlay>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function Connecting() {
  return (
    <div className="dbx-center">
      <div className="dbx-handoff" aria-hidden>
        <span className="app-tile">
          <Database />
        </span>
        <span className="dots">
          <i />
          <i />
          <i />
        </span>
        <span className="pulse">
          <DropboxMark size="lg" />
        </span>
      </div>
      <h3>Waiting for Dropbox</h3>
      <div className="msg">Allow keefast in the Dropbox page that opened in your browser, then come back here.</div>
      <div className="hstack mt-2">
        <Button
          onClick={() =>
            void api
              .dropboxReopenConnectPage()
              .then(() => toast("Dropbox opened in your browser again", { icon: ExternalLink }))
              .catch((failure) => toast(errorMessage(failure), { icon: "error" }))
          }
        >
          <ExternalLink /> Open the page again
        </Button>
        <Button variant="ghost" onClick={() => void api.dropboxCancelConnect()}>
          Stop
        </Button>
      </div>
    </div>
  );
}

function Connect({ error, onConnect }: { error: string | null; onConnect: () => void }) {
  return (
    <div className="dbx-center">
      <DropboxMark size="lg" />
      <h3>Connect your Dropbox</h3>
      <div className="msg">Connect your Dropbox account to open Vault files stored there.</div>
      <div className="dbx-privacy">
        <div>
          <ShieldCheck /> Dropbox asks you in your browser
        </div>
        <div>
          <ShieldCheck /> keefast never sees your Dropbox password
        </div>
        <div>
          <ShieldCheck /> Vault files stay encrypted end to end
        </div>
      </div>
      {error && (
        <div className="error-msg" role="alert">
          <CircleAlert className="ico-sm" /> {error}
        </div>
      )}
      <Button variant="primary" size="lg" className="mt-2" autoFocus onClick={onConnect}>
        <ExternalLink /> Connect Dropbox <Kbd combo="Enter" />
      </Button>
    </div>
  );
}

function AccountMenu({
  account,
  onUseAnother,
  onDisconnect,
}: {
  account: DropboxAccountView;
  onUseAnother: () => void;
  onDisconnect: () => void;
}) {
  const who = account.email || account.name;
  const initials =
    account.name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((word) => word[0]!.toUpperCase())
      .join("") || who.slice(0, 1).toUpperCase();
  return (
    <Menu>
      <MenuTrigger asChild>
        <button type="button" className="dbx-acct">
          <span className="av" aria-hidden>
            {initials}
          </span>
          <span className="truncate">Connected as {who}</span>
          <ChevronDown className="ico-sm" />
        </button>
      </MenuTrigger>
      <MenuContent align="end" className="w-[240px]">
        <MenuLabel>Connected as {account.name || who}</MenuLabel>
        <MenuItem icon={<Repeat />} onSelect={onUseAnother}>
          Use another account…
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={<Unplug />} danger onSelect={onDisconnect}>
          Disconnect
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}

function Files({
  listing,
  selected,
  listRef,
  onSelect,
  onPick,
  onUp,
  onRetry,
}: {
  listing: Listing;
  selected: string | null;
  listRef: React.RefObject<HTMLDivElement | null>;
  onSelect: (path: string) => void;
  onPick: (entry: DropboxEntry) => void;
  onUp: () => void;
  onRetry: () => void;
}) {
  const { entries, loading, error } = listing;
  if (loading && entries.length === 0) {
    return (
      <div className="dbx-files" aria-busy>
        {[140, 90, 170, 120, 100, 150].map((width, index) => (
          <div key={index} className="dbx-row">
            <span className="n">
              <span className="ficon">
                <span className="skel size-4 rounded-xs" />
              </span>
              <span className="skel" style={{ width }} />
            </span>
            <span className="skel w-[70px]" />
            <span className="skel ml-auto w-9" />
          </div>
        ))}
        <div className="dbx-loading">
          <LoaderCircle className="dbx-spin ico-sm" /> Loading…
        </div>
      </div>
    );
  }
  if (error) {
    return (
      <div className="dbx-files">
        <div className="dbx-center">
          <span className="ico">
            <CloudOff />
          </span>
          <div className="msg">{error}</div>
          <Button size="sm" onClick={onRetry}>
            <RotateCw /> Try again
          </Button>
        </div>
      </div>
    );
  }
  if (entries.length === 0) {
    return (
      <div className="dbx-files">
        <div className="dbx-center">
          <span className="ico">
            <FolderOpen />
          </span>
          <div className="msg">This folder is empty.</div>
          {listing.folder && (
            <Button variant="ghost" size="sm" onClick={onUp}>
              <ArrowUp /> Up
            </Button>
          )}
        </div>
      </div>
    );
  }
  return (
    <>
      <div className="dbx-fhead" aria-hidden>
        <span>Name</span>
        <span>Modified</span>
        <span className="text-right">Size</span>
      </div>
      <div ref={listRef} className={cn("dbx-files pt-0", loading && "opacity-60")} role="listbox" aria-label="Files and folders">
        {entries.map((entry) => {
          const vault = isVaultFile(entry);
          return (
            <button
              key={entry.path}
              type="button"
              role="option"
              aria-selected={selected === entry.path}
              data-row="1"
              data-path={entry.path}
              className={cn("dbx-row", selected === entry.path && "is-selected", !entry.isFolder && !vault && "other")}
              onClick={() => (entry.isFolder ? onPick(entry) : onSelect(entry.path))}
              onDoubleClick={() => !entry.isFolder && onPick(entry)}
            >
              <span className="n">
                <span className={cn("ficon", vault && "vault")}>{entry.isFolder ? <Folder /> : vault ? <FileLock2 /> : <File />}</span>
                <span className="truncate">
                  {vault ? (
                    <>
                      {entry.name.slice(0, -5)}
                      <span className="ext">{entry.name.slice(-5)}</span>
                    </>
                  ) : (
                    entry.name
                  )}
                </span>
                {entry.isFolder && (
                  <span className="chev">
                    <ChevronRight className="ico-sm" />
                  </span>
                )}
              </span>
              <span className="m">{entry.modified !== null ? modifiedLabel(entry.modified) : ""}</span>
              <span className="s">{entry.size !== null ? sizeLabel(entry.size) : ""}</span>
            </button>
          );
        })}
      </div>
    </>
  );
}
