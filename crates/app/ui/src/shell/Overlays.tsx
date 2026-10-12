// What floats over the shell, one at a time (`ShellState.overlay`): a confirmation, deleting for
// good (Confirms A's dialog, `screens/dialogs`), installing a Plugin (`screens/plugins`), an
// attachment's preview, and command palette A
// (`screens/command-palette`), which lists the commands the screens register (`useAppCommands`).

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import { Button, Dialog } from "@/kit";
import { CommandPalette } from "@/screens/command-palette/CommandPalette";
import { DeleteDialog } from "@/screens/dialogs/DeleteDialog";
import { InstallDialog, RefusedDialog } from "@/screens/plugins/InstallDialog";

import { type AttachmentOverlay, type ConfirmOverlay, useShell } from "./state";

export function Overlays() {
  const { state, actions } = useShell();
  const overlay = state.overlay;
  if (!overlay) return null;
  const close = (open: boolean) => !open && actions.closeOverlay();
  switch (overlay.kind) {
    case "confirm":
      return <Confirm overlay={overlay} onClose={actions.closeOverlay} onOpenChange={close} />;
    case "delete":
      return <DeleteDialog overlay={overlay} onClose={actions.closeOverlay} />;
    case "attachment":
      return <AttachmentPreview overlay={overlay} onOpenChange={close} />;
    case "palette":
      return <CommandPalette onClose={actions.closeOverlay} />;
    case "installPlugin":
      return <InstallDialog offer={overlay.offer} onClose={actions.closeOverlay} />;
    case "pluginRefused":
      return <RefusedDialog message={overlay.message} detail={overlay.detail} onClose={actions.closeOverlay} />;
  }
}

function Confirm({ overlay, onClose, onOpenChange }: { overlay: ConfirmOverlay; onClose: () => void; onOpenChange: (open: boolean) => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      onOpenChange={onOpenChange}
      title={overlay.title}
      description={overlay.description}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={overlay.danger ? "danger" : "primary"}
            disabled={busy}
            autoFocus
            onClick={async () => {
              setBusy(true);
              try {
                await overlay.onConfirm();
              } finally {
                onClose();
              }
            }}
          >
            {overlay.confirmLabel}
          </Button>
        </>
      }
    />
  );
}

function AttachmentPreview({ overlay, onOpenChange }: { overlay: AttachmentOverlay; onOpenChange: (open: boolean) => void }) {
  const preview = useQuery({
    queryKey: ["attachmentPreview", overlay.vault, overlay.entry, overlay.index],
    queryFn: () => api.previewAttachment(overlay.vault, overlay.entry, overlay.index),
    staleTime: 0,
    gcTime: 0,
  });
  const content = preview.data;
  return (
    <Dialog open onOpenChange={onOpenChange} title={overlay.name} className="w-[640px]">
      {preview.error ? (
        <div className="error-msg">{errorMessage(preview.error)}</div>
      ) : !content ? null : content.kind === "text" ? (
        <>
          <pre className="vb-preview">{content.text}</pre>
          {content.cut && <div className="hint">Only the start of the file shows here. Save it to see the rest.</div>}
        </>
      ) : content.kind === "image" ? (
        <img className="vb-preview-img" src={content.dataUrl} alt={overlay.name} />
      ) : (
        <div className="hint">keefast can't show this file. Save it to open it.</div>
      )}
    </Dialog>
  );
}
