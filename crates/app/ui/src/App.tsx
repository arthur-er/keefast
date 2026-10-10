// The app's root: theme, backend events, global shortcuts, and which screen shows. The screens
// here are placeholders proving the round trip; PR 2 replaces them with the prototype's shell
// (Vault browser C) and Open a Vault B.

import { useEffect, useState } from "react";

import { api, errorMessage, onEvent } from "@/api";
import { useBackendEvents, useSettings, useWorkspace } from "@/api/queries";
import { Dialog, dismissToast, Kbd, toast, Toaster } from "@/kit";
import { useActivityReporting } from "@/lib/activity";
import { SHORTCUTS, useShortcut } from "@/lib/shortcuts";
import { useApplyTheme } from "@/lib/theme";
import { BrowserScreen } from "@/screens/Browser";
import { PendingRequests } from "@/screens/PendingRequests";
import { UnlockScreen } from "@/screens/Unlock";

export function App() {
  useBackendEvents();
  useActivityReporting();
  const settings = useSettings();
  useApplyTheme(settings.data?.theme);
  const workspace = useWorkspace();
  const [showUnlock, setShowUnlock] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);

  useEffect(() => {
    const stops = [
      onEvent("mergeNotice", (notice) => toast(notice.message, { icon: "info", duration: 15000 })),
      onEvent("saveFailed", (failed) =>
        toast(`${failed.vaultName}: ${failed.message}`, { icon: "error", duration: 8000 }),
      ),
      onEvent("clipboardCleared", () => dismissToast()),
    ];
    return () => stops.forEach((stop) => stop());
  }, []);

  useShortcut(SHORTCUTS.palette, () => setPaletteOpen((open) => !open));
  useShortcut(SHORTCUTS.lockAll, () => void api.lockAll().catch((e) => toast(errorMessage(e), { icon: "error" })));
  useShortcut(SHORTCUTS.save, () => void api.saveAll());
  useShortcut(SHORTCUTS.openVault, () => setShowUnlock(true));

  const snapshot = workspace.data;
  if (!snapshot) {
    return workspace.error ? <div className="p-6 text-danger">{errorMessage(workspace.error)}</div> : null;
  }
  const anyUnlocked = snapshot.vaults.some((vault) => vault.state !== "locked");
  return (
    <>
      {showUnlock || !anyUnlocked ? (
        <UnlockScreen
          snapshot={snapshot}
          onDone={() => setShowUnlock(false)}
          onCancel={anyUnlocked ? () => setShowUnlock(false) : undefined}
        />
      ) : (
        <BrowserScreen snapshot={snapshot} onOpenAnother={() => setShowUnlock(true)} />
      )}
      <PendingRequests snapshot={snapshot} />
      <Dialog
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        title="Command palette"
        description="The command palette arrives with the next screens. Its shortcuts already work."
      >
        <div className="hstack muted">
          <Kbd combo="Ctrl K" /> or <Kbd combo="Ctrl Shift P" /> opens and closes it.
        </div>
      </Dialog>
      <Toaster />
    </>
  );
}
