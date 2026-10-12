// Installing a Plugin from a package file (ADR 0009): the file dialog, then what the package asks
// for, in plain words, before anything is installed (`ShellState.overlay` kind `installPlugin`), or
// why keefast refuses it (`pluginRefused`). An update says what it asks for that wasn't agreed to
// before.

import { useState } from "react";
import { ClipboardCopy, Eye, MousePointerClick, Puzzle, ShieldAlert, ShieldCheck, ShieldX, TextCursorInput, TriangleAlert, WifiOff } from "lucide-react";

import { api, ApiError, errorMessage } from "@/api";
import type { PluginOffer } from "@/bindings";
import { Badge, Button, toast } from "@/kit";
import { cn } from "@/lib/utils";
import { DialogFrame, Tile } from "@/screens/dialogs/DialogFrame";
import type { ShellActions } from "@/shell/state";

import { type CapabilityKind, capabilityLines } from "./capabilities";

import "@/screens/dialogs/dialogs.css";
import "./plugins.css";

/** Asks for a package, then shows what it asks for, or why it can't be installed. */
export async function installFromFile(actions: ShellActions) {
  try {
    const offer = await api.choosePluginPackage();
    if (offer) actions.openOverlay({ kind: "installPlugin", offer });
  } catch (error) {
    if (error instanceof ApiError && error.kind === "cancelled") return;
    actions.openOverlay({
      kind: "pluginRefused",
      message: errorMessage(error),
      detail: error instanceof ApiError ? error.detail : null,
    });
  }
}

export const CAPABILITY_ICONS: Record<CapabilityKind, typeof Eye> = {
  ownedFields: TextCursorInput,
  readProtected: Eye,
  copy: ClipboardCopy,
  browserFill: MousePointerClick,
};

/** A Plugin's icon, or a puzzle piece. */
export function PluginIcon({ icon, size = 36 }: { icon: string | null; size?: number }) {
  return icon ? (
    <img className="pl-icon" src={icon} alt="" width={size} height={size} />
  ) : (
    <span className="pl-icon pl-icon-none" style={{ width: size, height: size }}>
      <Puzzle />
    </span>
  );
}

export function InstallDialog({ offer, onClose }: { offer: PluginOffer; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const update = offer.installedVersion !== null;
  const lines = capabilityLines(
    offer,
    update ? { capabilities: offer.newCapabilities, ownedFields: offer.newOwnedFields, filledFields: offer.newFilledFields } : undefined,
  );
  const asksMore = offer.newCapabilities.length > 0 || offer.newOwnedFields.length > 0 || offer.newFilledFields.length > 0;
  const publisher = offer.publisher.kind === "unverified" ? offer.publisher : null;
  const keefast = offer.publisher.kind === "keefast" ? offer.publisher : null;
  const file = offer.path.split(/[\\/]/).pop();

  const install = async () => {
    setBusy(true);
    try {
      await api.installPlugin(offer.path, offer.digest);
      toast(
        <>
          {update ? "Updated" : "Installed"} <b>{offer.name}</b>
        </>,
        { icon: "check" },
      );
      onClose();
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
      setBusy(false);
    }
  };

  return (
    <DialogFrame
      width={480}
      lead={<PluginIcon icon={offer.icon} />}
      title={update ? `Update “${offer.name}” to ${offer.version}?` : `Install “${offer.name}”?`}
      description={
        <>
          {update ? (
            <>
              From <b>{file}</b>. Version {offer.installedVersion} is installed.
            </>
          ) : (
            <>
              Version {offer.version}, from <b>{file}</b>
            </>
          )}
        </>
      }
      onEscape={onClose}
      hint={
        <>
          <WifiOff /> Plugins have no network or file access.
        </>
      }
      actions={
        <>
          <Button onClick={onClose} shortcut="Esc">
            Cancel
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void install()}>
            {update ? "Update" : "Install"}
          </Button>
        </>
      }
    >
      {keefast && (
        <div className="pl-publisher" role="note">
          <ShieldCheck />
          <div>
            <div className="t">
              Signed by <b>keefast</b>
            </div>
            <div className="fp">Key {keefast.fingerprint}</div>
          </div>
        </div>
      )}
      {publisher &&
        (offer.newKey ? (
          <div className="pl-publisher warn" role="note">
            <ShieldAlert />
            <div>
              <div className="t">
                Unverified publisher: <b>{publisher.name}</b>
              </div>
              <div className="u">
                keefast can't confirm who made this Plugin. It hasn't seen their key before, and from now on will only accept updates of “{offer.name}” signed with it. Install it only if you trust where
                the file came from.
              </div>
              <div className="fp">Key {publisher.fingerprint}</div>
            </div>
          </div>
        ) : (
          <div className="pl-publisher" role="note">
            <ShieldCheck />
            <div>
              <div className="t">
                Signed with the same key as the installed version: <b>{publisher.name}</b>
              </div>
              <div className="fp">Key {publisher.fingerprint}</div>
            </div>
          </div>
        ))}
      <div className="pl-asks-head">{update ? (asksMore ? "This update asks for more. It may:" : "It asks for nothing new. It may still:") : "If you install it, it may:"}</div>
      <ul className="pl-asks">
        {lines.map((line) => {
          const Icon = CAPABILITY_ICONS[line.kind];
          return (
            <li key={line.kind} className={cn(update && !line.isNew && "is-old")}>
              <Icon />
              <div className="min-w-0">
                <div className="t">
                  {line.title}
                  {line.isNew && <Badge tone="warn">New</Badge>}
                </div>
                <div className="u">{line.desc}</div>
              </div>
            </li>
          );
        })}
      </ul>
    </DialogFrame>
  );
}

export function RefusedDialog({ message, detail, onClose }: { message: string; detail: string | null; onClose: () => void }) {
  return (
    <DialogFrame
      role="alertdialog"
      width={440}
      lead={
        <Tile tone="danger">
          <ShieldX />
        </Tile>
      }
      title="This Plugin can't be installed"
      description={message}
      onEscape={onClose}
      actions={
        <Button variant="primary" autoFocus onClick={onClose} shortcut="↵">
          OK
        </Button>
      }
    >
      {detail && (
        <div className="pl-refusal">
          <TriangleAlert />
          <span>{detail}</span>
        </div>
      )}
    </DialogFrame>
  );
}
