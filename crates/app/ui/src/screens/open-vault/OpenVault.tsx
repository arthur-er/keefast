// Open a Vault B: the Vault files on a rail (locked Vaults, recent Vault files), and the chosen
// one's unlock pane, or its New Vault state (`NewVault.tsx`). A Vault file in Dropbox is chosen
// in Open from Dropbox A, a dialog over it.

import { useEffect, useState } from "react";
import {
  CircleAlert, Cloud, Eye, EyeOff, FileKey2, FileSearch, FolderOpen, HardDrive, KeyRound, LoaderCircle, Lock, Plus,
  Settings, ShieldCheck, X,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import { ApiError, api, errorMessage } from "@/api";
import type { Location, LocationView, WorkspaceSnapshot } from "@/bindings";
import { keys, useLaunch } from "@/api/queries";
import { Badge, Button, Kbd, Tooltip } from "@/kit";
import { useShortcut } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { DropboxDialog } from "@/screens/dropbox/DropboxDialog";
import type { OpenVaultMode } from "@/shell/state";

import { NewVault } from "./NewVault";
import "./open-vault.css";

interface Choice extends LocationView {
  locked: boolean;
}

const sameLocation = (a: Location | null, b: Location | null) => !!a && !!b && a.provider === b.provider && a.path === b.path;

/** A Vault file chosen from disk, named as the Vault would be. */
export function locationOfPath(path: string): LocationView {
  const file = path.split(/[\\/]/).pop() ?? path;
  return { provider: "local", path, name: file.replace(/\.kdbx$/i, ""), description: path };
}

/** A Vault file in Dropbox, named as the Vault would be. */
export function dropboxLocation(path: string): LocationView {
  return { ...locationOfPath(path), provider: "dropbox", path, description: `Dropbox: ${path}` };
}

export function OpenVault({
  snapshot,
  mode,
  onModeChange,
  onDone,
  onClose,
  onSettings,
}: {
  snapshot: WorkspaceSnapshot;
  mode: OpenVaultMode;
  onModeChange: (mode: OpenVaultMode) => void;
  onDone: () => void;
  /** Back to the open Vaults; none when no Vault is unlocked. */
  onClose?: () => void;
  /** To the settings, which show in the shell: none when no Vault is unlocked. */
  onSettings?: () => void;
}) {
  const launch = useLaunch().data;
  const locked: Choice[] = snapshot.vaults.filter((vault) => vault.state === "locked").map((vault) => ({ ...vault.location, locked: true }));
  const recents: Choice[] = snapshot.recentVaults.map((recent) => ({ ...recent, locked: false }));
  const [chosen, setChosen] = useState<LocationView | null>(null);
  const [launchKeyFile, setLaunchKeyFile] = useState<string | null>(null);
  const [dropboxOpen, setDropboxOpen] = useState(false);
  const dropbox = useQuery({ queryKey: keys.dropbox, queryFn: api.dropboxStatus }).data;

  // Starts on the Vault file the command line named, else the locked Vault to unlock, else the
  // most recent Vault file.
  useEffect(() => {
    if (chosen) return;
    if (launch?.vaultFile) {
      const known = [...locked, ...recents].find((choice) => sameLocation(choice, launch.vaultFile));
      setChosen(known ?? locationOfPath(launch.vaultFile.path));
      setLaunchKeyFile(launch.keyFile);
      return;
    }
    const toUnlock = snapshot.vaults.find((vault) => vault.id === snapshot.vaultToUnlock && vault.state === "locked");
    const first = toUnlock?.location ?? locked[0] ?? recents[0];
    if (first) setChosen(first);
    // Only the first choice is automatic: later snapshots don't move it.
  }, [launch]);

  const chooseFile = async () => {
    const path = await api.chooseVaultFile().catch(() => null);
    if (!path) return;
    const known = [...locked, ...recents].find((choice) => choice.provider === "local" && choice.path === path);
    setChosen(known ?? locationOfPath(path));
    onModeChange("unlock");
  };
  useShortcut("Ctrl O", () => void chooseFile(), { enabled: !dropboxOpen });
  useShortcut("Ctrl N", () => onModeChange("new"), { enabled: !dropboxOpen });
  useShortcut("Escape", () => (mode === "new" ? onModeChange("unlock") : onClose?.()), {
    enabled: !dropboxOpen && (mode === "new" || !!onClose),
  });

  const item = (choice: Choice) => (
    <div
      key={`${choice.provider}:${choice.path}`}
      role="option"
      tabIndex={0}
      aria-selected={mode === "unlock" && sameLocation(chosen, choice)}
      className={cn("b-item", mode === "unlock" && sameLocation(chosen, choice) && "is-active")}
      onClick={() => {
        setChosen(choice);
        onModeChange("unlock");
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          setChosen(choice);
          onModeChange("unlock");
        }
      }}
    >
      <VaultTile location={choice} locked={choice.locked} />
      <span className="txt">
        <span className="nm truncate">
          {choice.name}
          {choice.locked && <Badge>Locked</Badge>}
        </span>
        <span className="path truncate">{choice.description}</span>
      </span>
      {!choice.locked && (
        <button
          type="button"
          className="btn ghost icon sm forget"
          aria-label={`Forget ${choice.name}`}
          title="Forget this Vault file"
          onClick={(event) => {
            event.stopPropagation();
            if (sameLocation(chosen, choice)) setChosen(null);
            void api.forgetRecent({ provider: choice.provider, path: choice.path });
          }}
        >
          <X />
        </button>
      )}
    </div>
  );

  const chosenLocked = chosen ? locked.some((choice) => sameLocation(choice, chosen)) : false;

  return (
    <div className="b-shell">
      <aside className="b-rail" aria-label="Vault files">
        <div className="b-rail-top">
          <span className="brand">
            <KeyRound />
          </span>
          keefast
        </div>
        <div className="b-rail-scroll" role="listbox" aria-label="Vault files">
          {locked.length > 0 && (
            <div className="b-sec">
              <div className="nav-head">Locked Vaults</div>
              {locked.map(item)}
            </div>
          )}
          <div className="b-sec">
            <div className="nav-head">Recent Vault files</div>
            {recents.length ? recents.map(item) : <div className="hint px-2 py-1">Vault files you open appear here.</div>}
          </div>
        </div>
        <div className="b-rail-foot">
          <button type="button" className="nav-item w-full text-left" onClick={() => void chooseFile()}>
            <FolderOpen /> Choose Vault file… <Kbd combo="Ctrl O" />
          </button>
          {dropbox?.available && (
            <button type="button" className="nav-item w-full text-left" onClick={() => setDropboxOpen(true)}>
              <Cloud /> From Dropbox…
            </button>
          )}
          <button
            type="button"
            className={cn("nav-item w-full text-left", mode === "new" && "is-selected")}
            onClick={() => onModeChange("new")}
          >
            <Plus /> New Vault… <Kbd combo="Ctrl N" />
          </button>
        </div>
      </aside>
      <main className="b-main">
        <div className="toolbar">
          <div className="crumbs">
            <b>{mode === "new" ? "New Vault" : "Open a Vault"}</b>
          </div>
          <div className="hstack gap-0.5">
            {onSettings && (
              <Tooltip content="Settings" shortcut="Ctrl ,">
                <button type="button" className="btn ghost icon sm" aria-label="Settings" onClick={onSettings}>
                  <Settings />
                </button>
              </Tooltip>
            )}
            {onClose && (
              <Tooltip content="Back to the open Vaults" shortcut="Escape">
                <button type="button" className="btn ghost icon sm" aria-label="Back to the open Vaults" onClick={onClose}>
                  <X />
                </button>
              </Tooltip>
            )}
          </div>
        </div>
        {mode === "new" ? (
          <NewVault active={!dropboxOpen} onDone={onDone} onCancel={() => onModeChange("unlock")} />
        ) : chosen ? (
          <UnlockPane
            key={`${chosen.provider}:${chosen.path}`}
            location={chosen}
            locked={chosenLocked}
            initialKeyFile={sameLocation(chosen, launch?.vaultFile ?? null) ? launchKeyFile : null}
            onDone={onDone}
          />
        ) : (
          <div className="b-stage">
            <div className="b-empty">
              <span className="vtile xl">
                <FileSearch />
              </span>
              <div className="font-medium text-fg-0">No Vault file selected</div>
              <div className="hint">Pick a Vault file on the left, or open one from disk.</div>
              <Button className="mt-1.5" onClick={() => void chooseFile()} shortcut="Ctrl O">
                <FolderOpen /> Choose Vault file…
              </Button>
            </div>
          </div>
        )}
      </main>
      {dropboxOpen && (
        <DropboxDialog
          onClose={() => setDropboxOpen(false)}
          onPick={(location) => {
            setDropboxOpen(false);
            const known = [...locked, ...recents].find((choice) => sameLocation(choice, location));
            setChosen(known ?? dropboxLocation(location.path));
            onModeChange("unlock");
          }}
        />
      )}
    </div>
  );
}

function VaultTile({ location, locked, xl }: { location: LocationView; locked: boolean; xl?: boolean }) {
  const pip = locked ? <Lock /> : location.provider === "dropbox" ? <Cloud /> : null;
  return (
    <span className={cn("vtile", xl && "xl")} aria-hidden>
      {(location.name.trim()[0] ?? "?").toUpperCase()}
      {pip && <span className="pip">{pip}</span>}
    </span>
  );
}

function UnlockPane({
  location,
  locked,
  initialKeyFile,
  onDone,
}: {
  location: LocationView;
  locked: boolean;
  initialKeyFile: string | null;
  onDone: () => void;
}) {
  const [password, setPassword] = useState("");
  const [shown, setShown] = useState(false);
  const [keyFile, setKeyFile] = useState<string | null>(initialKeyFile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [shake, setShake] = useState(0);

  const unlock = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const unlocked = await api.unlockVault({ provider: location.provider, path: location.path }, { password, keyFile });
      setPassword("");
      // `null`: the OS locked while the key was derived, so the Vault stays locked, to unlock again.
      if (unlocked !== null) onDone();
    } catch (failure) {
      const apiError = failure instanceof ApiError ? failure : new ApiError({ kind: "unavailable", message: errorMessage(failure), detail: null });
      setError(apiError);
      if (apiError.kind === "wrongKey") setShake((count) => count + 1);
    } finally {
      setBusy(false);
    }
  };

  const dropbox = location.provider === "dropbox";
  return (
    <div className="b-stage">
      <form
        className="b-pane"
        onSubmit={(event) => {
          event.preventDefault();
          void unlock();
        }}
      >
        <div className="b-id">
          <VaultTile location={location} locked={locked} xl />
          <h2>{location.name}</h2>
          <div className="path">{location.description}</div>
          <div className="facts">
            {locked && (
              <Badge>
                <Lock className="ico-sm" /> This Vault is locked
              </Badge>
            )}
            <Badge tone={dropbox ? "info" : "neutral"}>
              {dropbox ? <Cloud className="ico-sm" /> : <HardDrive className="ico-sm" />} {dropbox ? "Dropbox" : "On this device"}
            </Badge>
          </div>
        </div>
        <div className="b-form">
          <div className="field">
            <label htmlFor="master-password">Master password</label>
            <div key={shake} className={cn("pw", shake > 0 && "shake")}>
              <div className={cn("input lg pr-1", error?.kind === "wrongKey" && "invalid")}>
                <input
                  id="master-password"
                  type={shown ? "text" : "password"}
                  className={cn(shown && "revealed")}
                  value={password}
                  placeholder="Type your master password"
                  autoFocus
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                  onChange={(event) => setPassword(event.target.value)}
                />
                <button
                  type="button"
                  className="btn ghost icon sm"
                  aria-label={shown ? "Hide password" : "Show password"}
                  title={shown ? "Hide password" : "Show password"}
                  onClick={() => setShown(!shown)}
                >
                  {shown ? <EyeOff /> : <Eye />}
                </button>
              </div>
            </div>
          </div>
          <div className="field">
            <label>Key file</label>
            <div className={cn("b-keyfile", keyFile && "has")}>
              <FileKey2 className="ico-sm" />
              {keyFile ? (
                <>
                  <span className="nm truncate" title={keyFile}>
                    {keyFile}
                  </span>
                  <span className="spacer" />
                  <button type="button" className="btn ghost icon sm" aria-label="Remove the Key file" onClick={() => setKeyFile(null)}>
                    <X />
                  </button>
                </>
              ) : (
                <>
                  <span className="text-sm">None (optional)</span>
                  <span className="spacer" />
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={async () => {
                      const picked = await api.chooseKeyFile().catch(() => null);
                      if (picked) setKeyFile(picked);
                    }}
                  >
                    Choose Key file…
                  </Button>
                </>
              )}
            </div>
          </div>
          {busy ? (
            <div className="vstack gap-2">
              <div className="progress">
                <i />
              </div>
              <div className="hint">Deriving the key. This can take a few seconds.</div>
            </div>
          ) : (
            error && (
              <div className="notice danger" role="alert">
                <CircleAlert />
                <span>
                  {error.message}
                  {error.detail && <span className="detail">{error.detail}</span>}
                </span>
              </div>
            )
          )}
          <div className="b-actions">
            <Button type="submit" variant="primary" size="lg" className="min-w-[140px]" disabled={busy}>
              {busy ? (
                <>
                  <LoaderCircle className="spin" /> Unlocking…
                </>
              ) : (
                <>
                  Unlock <Kbd combo="Enter" />
                </>
              )}
            </Button>
            <span className="hint-r">
              <ShieldCheck className="ico-sm" /> Decrypted only in memory
            </span>
          </div>
        </div>
      </form>
    </div>
  );
}
