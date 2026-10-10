// Placeholder: a plain unlock form proving the round trip. PR 2 replaces it with Open a Vault B
// (Vault files rail + unlock pane).

import { useEffect, useState } from "react";
import { FileKey, FolderOpen, LoaderCircle, Lock, Plus, X } from "lucide-react";

import { ApiError, api, errorMessage } from "@/api";
import type { Location, WorkspaceSnapshot } from "@/api";
import { useLaunch } from "@/api/queries";
import { Button, Field, IconButton, Input, PasswordInput } from "@/kit";
import { cn } from "@/lib/utils";

export function UnlockScreen({
  snapshot,
  onDone,
  onCancel,
}: {
  snapshot: WorkspaceSnapshot;
  onDone: () => void;
  onCancel?: () => void;
}) {
  const launch = useLaunch();
  const locked = snapshot.vaults.filter((vault) => vault.state === "locked");
  const toUnlock = locked.find((vault) => vault.id === snapshot.vaultToUnlock) ?? locked[0];
  const [location, setLocation] = useState<Location | null>(null);
  const [password, setPassword] = useState("");
  const [keyFile, setKeyFile] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [creating, setCreating] = useState(false);

  // Starts on the Vault file the command line named, else the locked Vault to unlock, else the
  // most recent Vault file.
  useEffect(() => {
    if (location) return;
    if (launch.data?.vaultFile) {
      setLocation(launch.data.vaultFile);
      setKeyFile(launch.data.keyFile);
    } else if (toUnlock) {
      setLocation({ provider: toUnlock.location.provider, path: toUnlock.location.path });
    } else if (snapshot.recentVaults[0]) {
      const recent = snapshot.recentVaults[0];
      setLocation({ provider: recent.provider, path: recent.path });
    }
  }, [launch.data, toUnlock, snapshot.recentVaults, location]);

  async function unlock() {
    if (!location || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.unlockVault(location, { password, keyFile });
      setPassword("");
      onDone();
    } catch (failure) {
      setError(failure instanceof ApiError ? failure : new ApiError({ kind: "unavailable", message: errorMessage(failure), detail: null }));
    } finally {
      setBusy(false);
    }
  }

  const choices = [
    ...locked.map((vault) => ({ ...vault.location, locked: true })),
    ...snapshot.recentVaults.map((recent) => ({ ...recent, locked: false })),
  ];

  if (creating) return <NewVaultForm onDone={onDone} onCancel={() => setCreating(false)} />;

  return (
    <div className="grid h-full place-items-center bg-bg-0">
      <form
        className="card w-[440px]"
        onSubmit={(event) => {
          event.preventDefault();
          void unlock();
        }}
      >
        <div className="card-section vstack">
          <div className="hstack">
            <h1 className="m-0 text-lg font-semibold">Open a Vault</h1>
            <span className="spacer" />
            {onCancel && <IconButton label="Back to the open Vaults" onClick={onCancel}><X /></IconButton>}
          </div>
          {choices.length > 0 && (
            <div className="vstack gap-0.5" role="listbox" aria-label="Vault files">
              {choices.map((choice) => {
                const selected = location?.path === choice.path && location.provider === choice.provider;
                return (
                  <div
                    key={`${choice.provider}:${choice.path}`}
                    role="option"
                    aria-selected={selected}
                    className={cn("row", selected && "is-selected")}
                    onClick={() => setLocation({ provider: choice.provider, path: choice.path })}
                  >
                    {choice.locked ? <Lock className="ico-sm text-fg-2" /> : <FileKey className="ico-sm text-fg-2" />}
                    <span className="truncate">{choice.name}</span>
                    <span className="meta truncate">{choice.description}</span>
                    {!choice.locked && (
                      <IconButton
                        size="sm"
                        label="Forget this Vault file"
                        onClick={(event) => {
                          event.stopPropagation();
                          void api.forgetRecent({ provider: choice.provider, path: choice.path });
                        }}
                      >
                        <X />
                      </IconButton>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          <div className="hstack">
            <span className="truncate muted">{location ? location.path : "No Vault file selected"}</span>
            <span className="spacer" />
            <Button
              size="sm"
              onClick={async () => {
                const picked = await api.chooseVaultFile();
                if (picked) setLocation({ provider: "local", path: picked });
              }}
            >
              <FolderOpen /> Choose Vault file…
            </Button>
          </div>
        </div>
        <div className="card-section vstack">
          <Field label="Master password" htmlFor="master-password" error={error?.message}>
            <PasswordInput
              id="master-password"
              autoFocus
              value={password}
              invalid={error?.kind === "wrongKey"}
              placeholder="Type your master password"
              onChange={(event) => setPassword(event.target.value)}
            />
          </Field>
          {error?.detail && <div className="hint">{error.detail}</div>}
          <div className="hstack">
            <span className="label">Key file</span>
            <span className="truncate subtle">{keyFile ?? "None (optional)"}</span>
            <span className="spacer" />
            {keyFile && (
              <IconButton size="sm" label="Clear the Key file" onClick={() => setKeyFile(null)}>
                <X />
              </IconButton>
            )}
            <Button
              size="sm"
              variant="ghost"
              onClick={async () => {
                const picked = await api.chooseKeyFile();
                if (picked) setKeyFile(picked);
              }}
            >
              Choose Key file…
            </Button>
          </div>
        </div>
        <div className="dialog-foot">
          <Button variant="ghost" onClick={() => setCreating(true)}>
            <Plus /> New Vault…
          </Button>
          <span className="spacer" />
          <Button type="submit" variant="primary" disabled={!location || busy}>
            {busy ? <LoaderCircle className="animate-spin" /> : null}
            {busy ? "Unlocking…" : "Unlock"}
          </Button>
        </div>
      </form>
    </div>
  );
}

function NewVaultForm({ onDone, onCancel }: { onDone: () => void; onCancel: () => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    if (password !== confirm) return setError("The two master passwords are different.");
    setBusy(true);
    setError(null);
    try {
      const encryption = await api.defaultEncryption();
      await api.createVault({ path, name, key: { password, keyFile: null }, encryption });
      onDone();
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="grid h-full place-items-center bg-bg-0">
      <form
        className="card w-[440px]"
        onSubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <div className="card-section vstack gap-3">
          <h1 className="m-0 text-lg font-semibold">New Vault</h1>
          <Field label="Vault file" htmlFor="new-path">
            <div className="hstack">
              <Input id="new-path" value={path} placeholder="e.g. ~/Vaults/Personal.kdbx" onChange={(e) => setPath(e.target.value)} />
              <Button
                onClick={async () => {
                  const picked = await api.chooseNewVaultFile(path || null);
                  if (picked) setPath(picked);
                }}
              >
                Browse…
              </Button>
            </div>
          </Field>
          <Field label="Name" htmlFor="new-name" hint="The Vault file's name if empty">
            <Input id="new-name" value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label="Master password" htmlFor="new-password">
            <PasswordInput id="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
          <Field label="Repeat the master password" htmlFor="new-confirm" error={error}>
            <PasswordInput id="new-confirm" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
          </Field>
        </div>
        <div className="dialog-foot">
          <Button variant="ghost" onClick={onCancel}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={busy}>
            {busy ? "Creating the Vault…" : "Create"}
          </Button>
        </div>
      </form>
    </div>
  );
}
