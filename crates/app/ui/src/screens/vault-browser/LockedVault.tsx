// A locked Vault where its Entries would be: its master password and Key file, to unlock it
// in place (prototype "Locked Vault card").

import { useState } from "react";
import { CircleAlert, FileKey, KeyRound, LoaderCircle, Lock, X } from "lucide-react";

import { ApiError, api, errorMessage } from "@/api";
import type { VaultSummary } from "@/bindings";
import { AffixInput, Button, Kbd } from "@/kit";
import { cn } from "@/lib/utils";

export function LockedVault({ vault }: { vault: VaultSummary }) {
  const [password, setPassword] = useState("");
  const [keyFile, setKeyFile] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  const unlock = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.unlockVault({ provider: vault.location.provider, path: vault.location.path }, { password, keyFile });
      setPassword("");
    } catch (failure) {
      setError(failure instanceof ApiError ? failure : new ApiError({ kind: "unavailable", message: errorMessage(failure), detail: null }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="vb-locked">
      <form
        className="vb-ucard"
        onSubmit={(event) => {
          event.preventDefault();
          void unlock();
        }}
      >
        <div className="vb-ucard-head">
          <span className="vb-ucard-ico">
            <Lock />
          </span>
          <div>
            <h2>{vault.name}</h2>
            <div className="subtle mt-0.5">This Vault is locked.</div>
          </div>
          <span className="file">{vault.location.description}</span>
        </div>
        <div className="field">
          <label htmlFor="locked-password">Master password</label>
          <AffixInput
            id="locked-password"
            type="password"
            inputSize="lg"
            autoFocus
            autoComplete="off"
            invalid={error?.kind === "wrongKey"}
            leading={<KeyRound />}
            placeholder="Type your master password"
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.target.value)}
          />
          {error && (
            <span className="error-msg">
              <CircleAlert className="ico-sm" />
              {error.message}
            </span>
          )}
        </div>
        <div className="field">
          <label>Key file</label>
          <div className={cn("vb-keyfile", keyFile && "has")}>
            <FileKey />
            {keyFile ? (
              <>
                <span className="nm truncate">{keyFile}</span>
                <span className="spacer" />
                <button type="button" className="btn ghost sm icon" aria-label="Remove the Key file" onClick={() => setKeyFile(null)}>
                  <X />
                </button>
              </>
            ) : (
              <>
                <span>None (optional)</span>
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
        <Button type="submit" variant="primary" size="lg" disabled={busy}>
          {busy ? (
            <>
              <LoaderCircle className="vb-spin" /> Unlocking…
            </>
          ) : (
            <>
              Unlock <Kbd combo="Enter" />
            </>
          )}
        </Button>
        <span className="hint text-center">
          {busy ? "Deriving the key. This can take a few seconds." : <><Kbd combo="Ctrl L" /> locks every Vault again</>}
        </span>
      </form>
    </div>
  );
}
