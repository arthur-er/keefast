// Browser requests & confirms A: what waits for the user's answer, as centred dialogs over
// everything (the command palette closes; other overlays wait under them). One at a time: a Vault
// locking with unsaved changes, else a Pairing request, an Access request, a captured login, in
// the order the GPUI app asked them. The backend brings keefast to the front when a browser asks.

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { CircleAlert, Folder, FolderPlus, Globe, Info, Link2, Lock, Shield, Timer } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { AskedEntry, LoginOption, LoginTarget, VaultSummary, WorkspaceSnapshot } from "@/bindings";
import { useEntry, useVaultSettings, useWorkspace } from "@/api/queries";
import { Button, GroupIcon, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { EntryAvatar } from "@/screens/vault-browser/EntryList";
import { useShell } from "@/shell/state";
import { vaultIcon } from "@/shell/vaultStatus";

import { DialogFrame, Kicker, Tile } from "./DialogFrame";
import { type FrontRequest, frontRequest, requestKey, sameTarget, splitNewGroup, stepTarget } from "./logic";

import "./dialogs.css";

export function RequestDialogs({ snapshot }: { snapshot: WorkspaceSnapshot }) {
  const request = frontRequest(snapshot);
  const key = request && requestKey(request);
  const { state, actions } = useShell();
  const palette = state.overlay?.kind === "palette";
  useEffect(() => {
    // The request is answered first; the palette doesn't wait under it.
    if (key && palette) actions.closeOverlay();
  }, [key, palette, actions]);
  if (!request) return null;
  return <RequestDialog key={key} request={request} vaults={snapshot.vaults} />;
}

function RequestDialog({ request, vaults }: { request: FrontRequest; vaults: VaultSummary[] }) {
  switch (request.kind) {
    case "lock":
      return <LockDialog lock={request.lock} vault={vaults.find((vault) => vault.id === request.lock.vault)} />;
    case "pairing":
      return <PairingDialog pairing={request.pairing} />;
    case "access":
      return <AccessDialog access={request.access} />;
    case "login":
      return <LoginDialog login={request.login} />;
  }
}

const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });

/** Save, Discard or (when the user asked for the lock) Cancel. Enter saves; Escape cancels when it
 * may. */
function LockDialog({ lock, vault }: { lock: NonNullable<WorkspaceSnapshot["pendingLock"]>; vault?: VaultSummary }) {
  const [busy, setBusy] = useState(false);
  const answer = (choice: "save" | "discard" | "cancel") => {
    setBusy(true);
    api
      .answerLock(lock.vault, choice)
      .catch(fail)
      .finally(() => setBusy(false));
  };
  const Icon = vault ? vaultIcon({ ...vault, state: "unlocked" }) : Lock;
  return (
    <DialogFrame
      role="alertdialog"
      width={440}
      tight
      lead={<Tile tone="warn">{lock.canCancel ? <Lock /> : <Timer />}</Tile>}
      title={`“${lock.vaultName}” has unsaved changes. Save them before locking?`}
      description={
        lock.canCancel
          ? "Discarding loses them."
          : "keefast is locking, and the Vault stays hidden until you choose. Discarding loses the changes."
      }
      onEscape={lock.canCancel ? () => answer("cancel") : undefined}
      actions={
        <>
          <Button variant="dangerGhost" className="mr-auto" disabled={busy} onClick={() => answer("discard")}>
            Discard
          </Button>
          {lock.canCancel && (
            <Button disabled={busy} onClick={() => answer("cancel")} shortcut="Esc">
              Cancel
            </Button>
          )}
          <Button variant="primary" disabled={busy} autoFocus onClick={() => answer("save")} shortcut="↵">
            Save and lock
          </Button>
        </>
      }
    >
      <div className="dg-subject">
        <span className="dg-gico">
          <Icon />
        </span>
        <div className="min-w-0">
          <div className="t truncate">{lock.vaultName}</div>
          {vault && <div className="u truncate">{vault.location.description}</div>}
        </div>
        <span className="badge warn">Unsaved</span>
      </div>
    </DialogFrame>
  );
}

/** Names the browser, or turns it down. Enter pairs, Escape denies. */
function PairingDialog({ pairing }: { pairing: NonNullable<WorkspaceSnapshot["pendingPairing"]> }) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const known = useVaultSettings(pairing.vault).data?.pairings ?? [];
  const field = useRef<HTMLInputElement>(null);
  const approve = async () => {
    setBusy(true);
    try {
      await api.approvePairing(pairing.id, name);
      toast(
        <>
          Paired <b>{name.trim()}</b> with “{pairing.vaultName}”
        </>,
        { icon: Link2 },
      );
    } catch (failure) {
      setError(errorMessage(failure));
      field.current?.focus();
    } finally {
      setBusy(false);
    }
  };
  const deny = () => void api.denyPairing(pairing.id).catch(fail);
  return (
    <DialogFrame
      role="alertdialog"
      width={460}
      lead={
        <Tile tone="accent">
          <Link2 />
        </Tile>
      }
      title="Pair a browser"
      description={
        <>
          A browser extension asks to pair with the Vault <b>“{pairing.vaultName}”</b>. If this was you, name this browser so you
          can recognise it later.
        </>
      }
      onEscape={deny}
      hint={
        <>
          <Info />
          It can ask for logins while the Vault is open.
        </>
      }
      actions={
        <>
          <Button onClick={deny}>Deny</Button>
          <Button variant="primary" disabled={busy} onClick={() => void approve()} shortcut="↵">
            Pair
          </Button>
        </>
      }
    >
      <div className="field">
        <label htmlFor="pairing-name">Browser name</label>
        <div className={cn("input", error && "invalid")}>
          <Globe className="ico-sm subtle" />
          <input
            ref={field}
            id="pairing-name"
            autoFocus
            autoComplete="off"
            spellCheck={false}
            value={name}
            placeholder="e.g. Chrome on my laptop"
            aria-invalid={error !== null}
            onChange={(event) => {
              setName(event.target.value);
              setError(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void approve();
              }
            }}
          />
        </div>
        {error ? (
          <div className="error-msg">
            <CircleAlert className="ico-sm" />
            <span>{error}</span>
          </div>
        ) : (
          known.length > 0 && (
            <div className="dg-known">
              Already paired:
              {known.map((known) => (
                <span key={known.name} className="badge">
                  {known.name}
                </span>
              ))}
            </div>
          )
        )}
      </div>
    </DialogFrame>
  );
}

/** Allow or Deny, remembered on the Entries if the user wants. Escape denies; nothing on the
 * keyboard allows, so a keystroke meant for something else never gives a site access. */
function AccessDialog({ access }: { access: NonNullable<WorkspaceSnapshot["pendingAccess"]> }) {
  const [remember, setRemember] = useState(false);
  const answer = (allow: boolean) => {
    void api
      .answerAccess(access.id, allow, remember)
      .then(() =>
        toast(
          <>
            {allow ? "Allowed" : "Denied"} <b>{access.site}</b>
            {remember && <span className="subtle"> · remembered</span>}
          </>,
          { icon: allow ? "check" : Shield },
        ),
      )
      .catch(fail);
  };
  const count = access.entries.length;
  return (
    <DialogFrame
      role="alertdialog"
      width={500}
      autoFocus={false}
      lead={<Kicker icon={<Globe />}>{["Browser request"]}</Kicker>}
      title="Allow access?"
      onEscape={() => answer(false)}
      hint={
        <>
          <Shield />
          Not signing in? Deny.
        </>
      }
      actions={
        <>
          <Button onClick={() => answer(false)} shortcut="Esc">
            Deny
          </Button>
          <Button variant="primary" onClick={() => answer(true)}>
            Allow
          </Button>
        </>
      }
    >
      <SiteAddress site={access.site} via={`asks for ${count} ${count === 1 ? "Entry" : "Entries"}`} />
      <p className="dialog-desc m-0">
        <b>{access.site}</b> asks for these Entries. Allow only if you are signing in there.
      </p>
      <div className="dg-list">
        {access.entries.map((entry) => (
          <AskedEntryRow key={`${entry.vault}:${entry.entry}`} entry={entry} />
        ))}
      </div>
      <button type="button" className="dg-checkrow" role="checkbox" aria-checked={remember} onClick={() => setRemember(!remember)}>
        <span className={cn("check", remember && "is-on")} />
        <span>
          Remember this decision for <b>{access.site}</b>
        </span>
      </button>
    </DialogFrame>
  );
}

function SiteAddress({ site, via }: { site: string; via: string }) {
  return (
    <div className="dg-addr">
      <span className="lockico">
        <Lock />
      </span>
      <span className="host truncate">{site}</span>
      <span className="via">{via}</span>
    </div>
  );
}

function AskedEntryRow({ entry }: { entry: AskedEntry }) {
  const detail = useEntry(entry.vault, entry.entry).data;
  return (
    <div className="li">
      {detail ? <EntryAvatar entry={detail} size={28} /> : <span className="dg-gico" />}
      <div className="min-w-0">
        <div className="t truncate">{entry.title}</div>
        <div className="u truncate">{entry.username}</div>
      </div>
      <span className="badge">{entry.vaultName}</span>
    </div>
  );
}

/** Where a captured login goes: an Entry to update, or a Group for a new Entry, the suggested
 * place chosen (the Browser group, or the Entry the extension picked). ↑ ↓ choose, Enter saves,
 * Escape cancels. */
function LoginDialog({ login }: { login: NonNullable<WorkspaceSnapshot["pendingLogin"]> }) {
  const [chosen, setChosen] = useState<LoginTarget>(login.suggested);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector(".is-on")?.scrollIntoView({ block: "nearest" });
  }, [chosen]);
  const save = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await api.saveLogin(login.id, chosen);
      toast(
        <>
          Saved the login for <b>{login.site}</b>
        </>,
      );
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  };
  const cancel = () => void api.cancelLogin(login.id).catch(fail);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setChosen(stepTarget(login.options, chosen, event.key === "ArrowDown" ? 1 : -1));
      setError(null);
    } else if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      void save();
    }
  };
  return (
    <DialogFrame
      width={520}
      autoFocus={false}
      lead={<Kicker icon={<Globe />}>{["Captured login"]}</Kicker>}
      title="Save login?"
      description={
        <>
          You signed in to <b>{login.site}</b> as <b>“{login.username}”</b>. Choose where keefast saves this login.
        </>
      }
      onEscape={cancel}
      onKeyDown={onKeyDown}
      hint={
        <>
          <span className="kbds">
            <kbd>↑</kbd>
            <kbd>↓</kbd>
          </span>
          to choose
        </>
      }
      actions={
        <>
          <Button onClick={cancel} shortcut="Esc">
            Cancel
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void save()} shortcut="↵">
            Save
          </Button>
        </>
      }
    >
      <div ref={list} role="radiogroup" aria-label="Where to save the login" className={cn("tl", login.options.length > 9 && "scroll")}>
        {login.options.map((option, index) =>
          option.target ? (
            <TargetRow
              key={index}
              option={option}
              target={option.target}
              on={sameTarget(option.target, chosen)}
              onChoose={() => {
                setChosen(option.target!);
                setError(null);
              }}
            />
          ) : (
            <div key={index} className="tl-head">
              {option.label}
            </div>
          ),
        )}
      </div>
      {error && (
        <div className="error-msg">
          <CircleAlert className="ico-sm" />
          <span>{error}</span>
        </div>
      )}
    </DialogFrame>
  );
}

function TargetRow({ option, target, on, onChoose }: { option: LoginOption; target: LoginTarget; on: boolean; onChoose: () => void }) {
  const common = {
    role: "radio",
    "aria-checked": on,
    onClick: onChoose,
  } as const;
  if (target.kind === "update") {
    return (
      <div className={cn("tl-row tall", on && "is-on")} {...common}>
        <span className={cn("radio", on && "is-on")} />
        <UpdateLabel target={target} fallback={option.label} />
        {option.detail && <span className="badge">{option.detail}</span>}
      </div>
    );
  }
  const { name, isNew } = splitNewGroup(option.label);
  const icon = isNew ? <FolderPlus /> : option.icon ? <GroupIcon icon={option.icon} /> : <Folder />;
  return (
    <div className={cn("tl-row", on && "is-on")} style={{ paddingLeft: 8 + Math.max(0, option.depth - 1) * 20 }} {...common}>
      <span className={cn("radio", on && "is-on")} />
      {option.depth <= 1 ? <VaultGlyph vault={target.vault} /> : icon}
      <span className={cn("lbl truncate", option.depth <= 1 && "font-medium")}>
        {name}
        {isNew && <span className="subtle"> (new Group)</span>}
      </span>
      {option.detail && <span className="badge accent">{option.detail}</span>}
    </div>
  );
}

function VaultGlyph({ vault }: { vault: number }) {
  const summary = useWorkspace().data?.vaults.find((summary) => summary.id === vault);
  const Icon = summary ? vaultIcon(summary) : Lock;
  return <Icon />;
}

function UpdateLabel({ target, fallback }: { target: Extract<LoginTarget, { kind: "update" }>; fallback: string }) {
  const entry = useEntry(target.vault, target.entry).data;
  if (!entry) return <span className="lbl truncate">{fallback}</span>;
  return (
    <>
      <EntryAvatar entry={entry} size={26} />
      <div className="lbl truncate">
        <div className="t truncate">{entry.title}</div>
        <div className="u truncate">{entry.username}</div>
      </div>
    </>
  );
}
