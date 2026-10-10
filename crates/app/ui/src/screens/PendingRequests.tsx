// Placeholder dialogs for what waits for the user's answer: a Vault locking with unsaved changes,
// and the browsers' Pairing, Access and captured-login requests, asked one at a time in that
// order (as the GPUI app did). PR 7 gives them the prototype's design (dialogs A).

import { useState } from "react";

import { api, errorMessage } from "@/api";
import type { LoginTarget, WorkspaceSnapshot } from "@/api";
import { Button, Check, Dialog, Field, Input } from "@/kit";
import { cn } from "@/lib/utils";

export function PendingRequests({ snapshot }: { snapshot: WorkspaceSnapshot }) {
  const { pendingLock, pendingPairing, pendingAccess, pendingLogin } = snapshot;
  if (pendingLock) return <LockDialog key={`lock-${pendingLock.vault}`} lock={pendingLock} />;
  if (pendingPairing) return <PairingDialog key={`pairing-${pendingPairing.id}`} pairing={pendingPairing} />;
  if (pendingAccess) return <AccessDialog key={`access-${pendingAccess.id}`} access={pendingAccess} />;
  if (pendingLogin) return <LoginDialog key={`login-${pendingLogin.id}`} login={pendingLogin} />;
  return null;
}

function LockDialog({ lock }: { lock: NonNullable<WorkspaceSnapshot["pendingLock"]> }) {
  const answer = (choice: "save" | "discard" | "cancel") => void api.answerLock(lock.vault, choice);
  return (
    <Dialog
      open
      dismissible={lock.canCancel}
      onOpenChange={(open) => !open && answer("cancel")}
      title={`“${lock.vaultName}” has unsaved changes. Save them before locking?`}
      description={
        lock.canCancel
          ? "Discarding loses them."
          : "keefast is locking, and the Vault stays hidden until you choose. Discarding loses the changes."
      }
      footer={
        <>
          {lock.canCancel && <Button variant="ghost" onClick={() => answer("cancel")}>Cancel</Button>}
          <Button variant="dangerGhost" onClick={() => answer("discard")}>Discard</Button>
          <Button variant="primary" onClick={() => answer("save")} autoFocus>Save</Button>
        </>
      }
    />
  );
}

function PairingDialog({ pairing }: { pairing: NonNullable<WorkspaceSnapshot["pendingPairing"]> }) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const approve = async () => {
    try {
      await api.approvePairing(pairing.id, name);
    } catch (failure) {
      setError(errorMessage(failure));
    }
  };
  return (
    <Dialog
      open
      dismissible={false}
      title="Pair a browser"
      description={`A browser extension asks to pair with the Vault “${pairing.vaultName}”. If this was you, name this browser so you can recognise it later.`}
      footer={
        <>
          <Button variant="ghost" onClick={() => void api.denyPairing(pairing.id)}>Deny</Button>
          <Button variant="primary" onClick={() => void approve()}>Pair</Button>
        </>
      }
    >
      <Field label="Browser name" htmlFor="pairing-name" error={error}>
        <Input
          id="pairing-name"
          autoFocus
          value={name}
          placeholder="e.g. Chrome on my laptop"
          invalid={error !== null}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && void approve()}
        />
      </Field>
    </Dialog>
  );
}

function AccessDialog({ access }: { access: NonNullable<WorkspaceSnapshot["pendingAccess"]> }) {
  const [remember, setRemember] = useState(false);
  const answer = (allow: boolean) => void api.answerAccess(access.id, allow, remember);
  return (
    <Dialog
      open
      dismissible={false}
      title="Allow access?"
      description={`${access.site} asks for these Entries. Allow only if you are signing in there.`}
      footer={
        <>
          <Button variant="ghost" onClick={() => answer(false)}>Deny</Button>
          <Button variant="primary" onClick={() => answer(true)} autoFocus>Allow</Button>
        </>
      }
    >
      <div className="vstack gap-0.5">
        {access.entries.map((entry) => (
          <div key={`${entry.vault}:${entry.entry}`} className="row">
            <span className="truncate">{entry.title}</span>
            <span className="truncate text-fg-2">{entry.username}</span>
            <span className="meta">{entry.vaultName}</span>
          </div>
        ))}
      </div>
      <Check checked={remember} onCheckedChange={setRemember}>
        Remember this decision for {access.site}
      </Check>
    </Dialog>
  );
}

function LoginDialog({ login }: { login: NonNullable<WorkspaceSnapshot["pendingLogin"]> }) {
  const [chosen, setChosen] = useState<LoginTarget>(login.suggested);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    try {
      await api.saveLogin(login.id, chosen);
    } catch (failure) {
      setError(errorMessage(failure));
    }
  };
  return (
    <Dialog
      open
      dismissible={false}
      title="Save login?"
      description={`You signed in to ${login.site} as “${login.username}”. Choose where keefast saves this login.`}
      footer={
        <>
          <Button variant="ghost" onClick={() => void api.cancelLogin(login.id)}>Cancel</Button>
          <Button variant="primary" onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="vstack max-h-72 gap-0.5 overflow-auto">
        {login.options.map((option, index) =>
          option.target ? (
            <div
              key={index}
              className={cn("row", JSON.stringify(option.target) === JSON.stringify(chosen) && "is-selected")}
              style={{ paddingLeft: 10 + option.depth * 14 }}
              onClick={() => setChosen(option.target!)}
            >
              <span className="truncate">{option.label}</span>
              {option.detail && <span className="meta">{option.detail}</span>}
            </div>
          ) : (
            <div key={index} className="list-head">{option.label}</div>
          ),
        )}
      </div>
      {error && <div className="error-msg">{error}</div>}
    </Dialog>
  );
}
