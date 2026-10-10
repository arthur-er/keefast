// Changing an open Vault's master key and encryption, as Vault settings C draws it in its
// Security section (`vault-settings.html?variant=C&state=key`): a row that sums the encryption up
// with "Change…", which opens an inline card. The card starts from the Vault's own encryption and
// keeps its master key unless "Set a new master key" is on; Save changes gives the Vault the new
// settings and saves the Vault file straight away (`change_vault_key`; the Vault keeps its old key
// until the save has written the new one, ADR 0006).
//
// Vault settings C (Tauri UI 6/7) embeds `MasterKeySection` as its Security section's card, or
// `ChangeVaultKey` alone where it opens the card itself. Until then `VaultKeyCommand` (in the
// shell) reaches it from the command palette.

import { useEffect, useRef, useState } from "react";
import { CircleAlert, Eye, EyeOff, FileKey2, Gauge, Info, KeyRound, Plus, ShieldCheck, TriangleAlert, X } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { EncryptionSettings, VaultId } from "@/bindings";
import { useVaultSettings } from "@/api/queries";
import { Button, Dialog, Kbd, Switch, toast } from "@/kit";
import { useShortcut } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { useAppCommand } from "@/shell/commands";
import { useShell } from "@/shell/state";
import { useView } from "@/shell/view";

import { Choice } from "./EncryptionFields";
import { CIPHERS, checkEncryption, type EncryptionField, encryptionForm, type EncryptionForm, encryptionSummary, KDFS } from "./encryption";
import { checkKey, KEY_MESSAGES, matchState } from "./masterKey";
import { useKeyFile } from "./useKeyFile";
import "./vault-key.css";

/** The Security section's card: the row with the Vault's encryption and "Change…", or the
 * change-key card in its place. Put it under Vault settings' Security heading. */
export function MasterKeySection({ vault, initiallyOpen = false }: { vault: VaultId; initiallyOpen?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const settings = useVaultSettings(vault).data;
  return (
    <div className="vk set-card">
      {open ? (
        <ChangeVaultKey vault={vault} onClose={() => setOpen(false)} />
      ) : (
        <div className="set-row">
          <div className="txt">
            <div className="lbl">Master key and encryption</div>
            <div className="desc">{settings ? encryptionSummary(settings.encryption).join(" · ") : " "}</div>
          </div>
          <div className="ctl">
            <Button size="sm" onClick={() => setOpen(true)}>
              Change…
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/** The change-key card for `vault`. `onClose` after Cancel, Escape, or a save begun. Wrap it in
 * `.vk.set-card` (as `MasterKeySection` does) or anything with the `vk` class. */
export function ChangeVaultKey({ vault, onClose }: { vault: VaultId; onClose: () => void }) {
  const settings = useVaultSettings(vault).data;
  if (!settings) return null;
  // New Key files go by the Vault file, when it is on this machine.
  const near = settings.location.provider === "local" ? settings.location.path : null;
  return <ChangeKeyCard key={vault} vault={vault} name={settings.name} current={settings.encryption} near={near} onClose={onClose} />;
}

function ChangeKeyCard({
  vault,
  name,
  current,
  near,
  onClose,
}: {
  vault: VaultId;
  name: string;
  current: EncryptionSettings;
  near: string | null;
  onClose: () => void;
}) {
  const [setKey, setSetKey] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [shown, setShown] = useState(false);
  const keyFile = useKeyFile();
  // The form starts from the Vault's encryption as it was when the card opened.
  const [initial] = useState(current);
  const [encryption, setEncryption] = useState<EncryptionForm>(() => encryptionForm(current));
  const [tried, setTried] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const passwordRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (setKey) passwordRef.current?.focus();
  }, [setKey]);

  const match = setKey ? matchState(password, confirm) : "none";
  const checkedKey = checkKey(password, confirm, keyFile.path);
  const noKey = setKey && tried && !checkedKey.ok && checkedKey.message === KEY_MESSAGES.none;
  const checked = checkEncryption(encryption, initial);
  const encryptionError = checked.ok ? null : checked;
  const blocked = match === "mismatch" || !!encryptionError;

  const save = async () => {
    setTried(true);
    if (busy || blocked) return;
    if (setKey && !checkedKey.ok) return;
    if (!checked.ok) return;
    setBusy(true);
    setError(null);
    try {
      await api.changeVaultKey(vault, { newKey: setKey && checkedKey.ok ? checkedKey.key : null, encryption: checked.settings });
      // The master password is only needed until the Vault has it (ADR 0004).
      setPassword("");
      setConfirm("");
      toast(setKey ? "New master key saved to the Vault file" : "Encryption settings saved to the Vault file", { icon: ShieldCheck });
      onClose();
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  };

  useShortcut("Ctrl Enter", () => void save());

  const row = (label: string, control: React.ReactNode, field?: EncryptionField) => (
    <div className="set-row tight">
      <div className="txt">
        <label className="lbl" htmlFor={field && `vk-${field}`}>
          {label}
        </label>
      </div>
      <div className="ctl">{control}</div>
    </div>
  );
  const number = (field: EncryptionField, unit?: string) => (
    <div className={cn("input mono knum", encryptionError && (encryptionError.field === field || encryptionError.field === null) && "invalid")}>
      <input
        id={`vk-${field}`}
        inputMode="numeric"
        value={encryption[field]}
        disabled={busy}
        onChange={(event) => setEncryption({ ...encryption, [field]: event.target.value })}
      />
      {unit && <span className="unit">{unit}</span>}
    </div>
  );

  return (
    <form
      className="kc"
      aria-label={`Master key and encryption of ${name}`}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
      onKeyDown={(event) => {
        // Escape closes the card, not the page or dialog around it.
        if (event.key === "Escape" && !busy) {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="set-row kc-head">
        <span className="tile">
          <KeyRound />
        </span>
        <div className="txt">
          <div className="lbl">Master key and encryption of “{name}”</div>
          <div className="desc">Now: {encryptionSummary(initial).slice(0, 3).join(" · ")}</div>
        </div>
        <div className="ctl">
          <button type="button" className="btn sm ghost icon" aria-label="Cancel" title="Cancel (Esc)" onClick={onClose}>
            <X />
          </button>
        </div>
      </div>

      <div className="kc-sub">Master key</div>
      <div className="set-row">
        <div className="txt">
          <div className="lbl">Set a new master key</div>
          <div className="desc">Leave this off to keep the current master key and change only the encryption.</div>
        </div>
        <div className="ctl">
          <Switch checked={setKey} onCheckedChange={setSetKey} label="Set a new master key" disabled={busy} />
        </div>
      </div>
      <div data-part="key" className={cn(!setKey && "kdim")} aria-disabled={!setKey}>
        <div className="set-row">
          <div className="txt">
            <label className="lbl" htmlFor="vk-password">
              Master password
            </label>
          </div>
          <div className="ctl">
            <div className={cn("input mono kin", noKey && "invalid")}>
              <input
                ref={passwordRef}
                id="vk-password"
                type={shown ? "text" : "password"}
                value={password}
                placeholder="Type your master password"
                spellCheck={false}
                autoComplete="new-password"
                disabled={!setKey || busy}
                onChange={(event) => setPassword(event.target.value)}
              />
              <button
                type="button"
                className="eye"
                aria-label={shown ? "Hide the master password" : "Show the master password"}
                title={shown ? "Hide password" : "Show password"}
                disabled={!setKey}
                onClick={() => setShown(!shown)}
              >
                {shown ? <EyeOff className="ico-sm" /> : <Eye className="ico-sm" />}
              </button>
            </div>
          </div>
        </div>
        <div className="set-row">
          <div className="txt">
            <label className="lbl" htmlFor="vk-confirm">
              Repeat the master password
            </label>
            {match === "mismatch" && (
              <div className="desc">
                <span className="error-msg" role="alert">
                  <CircleAlert /> {KEY_MESSAGES.mismatch}
                </span>
              </div>
            )}
          </div>
          <div className="ctl">
            <div className={cn("input mono kin", match === "mismatch" && "invalid")}>
              <input
                id="vk-confirm"
                type={shown ? "text" : "password"}
                value={confirm}
                placeholder="Type it again"
                spellCheck={false}
                autoComplete="new-password"
                disabled={!setKey || busy}
                onChange={(event) => setConfirm(event.target.value)}
              />
            </div>
          </div>
        </div>
        <div className="set-row">
          <div className="txt">
            <div className="lbl">Key file</div>
            <div className="desc">
              <span className={cn("kfile-name", !keyFile.path && "none")} title={keyFile.path ?? undefined}>
                <FileKey2 className="ico-sm" />
                <span className="truncate">{keyFile.path ?? "None (optional)"}</span>
              </span>
              {noKey && (
                <span className="error-msg mt-1" role="alert">
                  <CircleAlert /> {KEY_MESSAGES.none}
                </span>
              )}
              {keyFile.error && (
                <span className="error-msg mt-1" role="alert">
                  <CircleAlert /> {keyFile.error}
                </span>
              )}
            </div>
          </div>
          <div className="ctl">
            {keyFile.path && (
              <button type="button" className="btn sm ghost icon" aria-label="Remove the Key file" title="Remove the Key file" onClick={keyFile.clear}>
                <X />
              </button>
            )}
            <Button size="sm" disabled={!setKey || busy} onClick={() => void keyFile.choose()}>
              Choose…
            </Button>
            <Button size="sm" disabled={!setKey || busy} onClick={() => void keyFile.create(near)}>
              <Plus /> Create new…
            </Button>
          </div>
        </div>
        <div className="set-note warn">
          <TriangleAlert />
          <span>Use a master password, a Key file, or both. Keep a copy of the Key file somewhere safe: without it the Vault can't be opened.</span>
        </div>
      </div>

      <div className="kc-sub">Encryption</div>
      {row(
        "Cipher",
        <Choice label="Cipher" full={false} options={CIPHERS} value={encryption.cipher} disabled={busy} onChange={(cipher) => setEncryption({ ...encryption, cipher })} />,
      )}
      {row(
        "Key derivation",
        <Choice label="Key derivation" full={false} options={KDFS} value={encryption.kdf} disabled={busy} onChange={(kdf) => setEncryption({ ...encryption, kdf })} />,
      )}
      {encryption.kdf === "aesKdf" ? (
        <div data-part="aes">{row("Rounds", number("rounds"), "rounds")}</div>
      ) : (
        <div data-part="argon">
          {row("Memory (MiB)", number("memoryMib", "MiB"), "memoryMib")}
          {row("Iterations", number("iterations"), "iterations")}
          {row("Parallelism", number("parallelism", "lanes"), "parallelism")}
        </div>
      )}
      <div className="set-note">
        <Gauge />
        <div>
          <div>Higher values make guessing the master password slower, and unlocking and saving too.</div>
          {encryptionError && (
            <div className="error-msg mt-1" role="alert">
              <CircleAlert /> {encryptionError.message}
            </div>
          )}
        </div>
      </div>

      <div className="kc-foot">
        {error ? (
          <span className="error-msg" role="alert">
            <CircleAlert /> {error}
          </span>
        ) : (
          <span className="hint">
            <Info className="ico-sm" /> keefast saves the Vault file as soon as you apply.
          </span>
        )}
        <span className="spacer" />
        <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" size="sm" disabled={busy || blocked}>
          Save changes <Kbd combo="Ctrl Enter" />
        </Button>
      </div>
    </form>
  );
}

/** For now, until Vault settings C: "Change master key…" in the command palette opens the
 * change-key card for the Vault shown, in a dialog. Vault settings C replaces this. */
export function VaultKeyCommand() {
  const { state } = useShell();
  const view = useView();
  const [vault, setVault] = useState<VaultId | null>(null);
  const scope = view?.scope;
  const shown = scope && (scope.kind === "vault" || scope.kind === "group") ? scope.vault : null;
  useAppCommand({
    id: "changeVaultKey",
    title: "Change master key and encryption…",
    section: "Vaults",
    icon: KeyRound,
    enabled: shown !== null,
    run: () => shown !== null && setVault(shown),
  }, state.overlay === null);
  if (vault === null) return null;
  return (
    <Dialog open onOpenChange={(open) => !open && setVault(null)} title="Master key and encryption" className="vk max-h-[calc(100vh-48px)] w-[680px] overflow-auto p-0 [&_.dialog-head]:sr-only [&_.dialog-body]:p-0">
      <div className="set-card">
        <ChangeVaultKey vault={vault} onClose={() => setVault(null)} />
      </div>
    </Dialog>
  );
}
