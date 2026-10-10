// What floats over the shell, one at a time (`ShellState.overlay`): a confirmation, an
// attachment's preview, and the commands list. Command palette A (Tauri UI 4/7) replaces the
// list; it reads the same registry (`useAppCommands`).

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import { Button, Dialog, Kbd } from "@/kit";

import { useAppCommands } from "./commands";
import { type AttachmentOverlay, type ConfirmOverlay, useShell } from "./state";

export function Overlays() {
  const { state, actions } = useShell();
  const overlay = state.overlay;
  if (!overlay) return null;
  const close = (open: boolean) => !open && actions.closeOverlay();
  switch (overlay.kind) {
    case "confirm":
      return <Confirm overlay={overlay} onClose={actions.closeOverlay} onOpenChange={close} />;
    case "attachment":
      return <AttachmentPreview overlay={overlay} onOpenChange={close} />;
    case "palette":
      return <CommandList onClose={actions.closeOverlay} onOpenChange={close} />;
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

/** Every command the screens registered, with its keys: until the palette arrives. */
function CommandList({ onClose, onOpenChange }: { onClose: () => void; onOpenChange: (open: boolean) => void }) {
  const commands = useAppCommands().filter((command) => !command.hidden);
  const sections = [...new Set(commands.map((command) => command.section))];
  return (
    <Dialog open onOpenChange={onOpenChange} title="Commands" description="The command palette arrives with a later screen; these are its commands.">
      <div className="vstack max-h-[420px] gap-0 overflow-auto">
        {sections.map((section) => (
          <div key={section}>
            <div className="menu-label">{section}</div>
            {commands
              .filter((command) => command.section === section)
              .map((command) => {
                const Icon = command.icon;
                const enabled = command.enabled ?? true;
                return (
                  <button
                    key={command.id}
                    type="button"
                    className="menu-item w-full text-left disabled:opacity-45"
                    disabled={!enabled}
                    onClick={() => {
                      onClose();
                      command.run();
                    }}
                  >
                    {Icon && <Icon />}
                    <span className="truncate">{command.title}</span>
                    {command.shortcut?.[0] && <Kbd combo={command.shortcut[0]} className="ml-auto" />}
                  </button>
                );
              })}
          </div>
        ))}
      </div>
    </Dialog>
  );
}
