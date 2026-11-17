// The app's root: theme, backend events, the app-wide commands, and which screen shows: Open a
// Vault B when no Vault is unlocked (or the user asked for it), else the shell (Vault browser C).
// The browsers' requests and a lock waiting on unsaved changes show over either (dialogs A).

import { useEffect } from "react";
import { Bot, FileUp, FolderOpen, Lock, Plus, Puzzle, Save, Settings } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { api, errorMessage, onEvent } from "@/api";
import { keys, useBackendEvents, useSettings, useWorkspace } from "@/api/queries";
import { dismissToast, toast, Toaster } from "@/kit";
import { useActivityReporting } from "@/lib/activity";
import { setChangedShortcuts, SHORTCUTS, useShortcut } from "@/lib/shortcuts";
import { useApplyTheme } from "@/lib/theme";
import { OpenVault } from "@/screens/open-vault/OpenVault";
import { RequestDialogs } from "@/screens/dialogs/RequestDialogs";
import { installFromFile } from "@/screens/plugins/InstallDialog";
import { useAppCommand } from "@/shell/commands";
import { hasPendingRequest } from "@/shell/hooks";
import { Overlays } from "@/shell/Overlays";
import { useQuitCommand } from "@/shell/quit";
import { Shell } from "@/shell/Shell";
import { ShellProvider, useShell } from "@/shell/state";

import "@/shell/shell.css";
import "@/screens/vault-browser/vault-browser.css";

export function App() {
  return (
    <ShellProvider>
      <Root />
    </ShellProvider>
  );
}

function Root() {
  useBackendEvents();
  useActivityReporting();
  const settings = useSettings();
  useApplyTheme(settings.data?.theme);
  // The keys the user gave shortcuts in Settings, for every key binding.
  const changedShortcuts = settings.data?.shortcuts;
  useEffect(() => setChangedShortcuts(changedShortcuts ?? {}), [changedShortcuts]);
  const workspace = useWorkspace();
  const { state, actions } = useShell();
  const snapshot = workspace.data;
  const anyUnlocked = snapshot?.vaults.some((vault) => vault.state !== "locked") ?? false;
  const openVaultShown = !anyUnlocked || state.openVault !== null;
  const keysActive = state.overlay === null && !hasPendingRequest(snapshot);
  const client = useQueryClient();

  useEffect(() => {
    const stops = [
      // keefast was launched again with a Vault file: offer it to unlock (ADR 0013).
      onEvent("launched", () => {
        void client.invalidateQueries({ queryKey: keys.launch });
        actions.showOpenVault("unlock");
      }),
      onEvent("mergeNotice", actions.showMerge),
      onEvent("saveFailed", (failed) => toast(`${failed.vaultName}: ${failed.message}`, { icon: "error", duration: 8000 })),
      onEvent("clipboardCleared", () => dismissToast()),
    ];
    return () => stops.forEach((stop) => stop());
  }, [actions, client]);

  const fail = (error: unknown) => toast(errorMessage(error), { icon: "error" });
  useShortcut(SHORTCUTS.palette, actions.togglePalette, { enabled: anyUnlocked && !openVaultShown && !hasPendingRequest(snapshot) });
  useShortcut(SHORTCUTS.close, actions.showBrowser, { enabled: keysActive && !openVaultShown && state.page.kind !== "browser" });
  useAppCommand({ id: "lockAll", title: "Lock all Vaults", section: "Vaults", icon: Lock, shortcut: SHORTCUTS.lockAll, enabled: anyUnlocked, run: () => void api.lockAll().catch(fail) }, keysActive);
  useAppCommand({ id: "saveAll", title: "Save all Vaults", section: "Vaults", icon: Save, shortcut: SHORTCUTS.save, enabled: anyUnlocked, run: () => void api.saveAll().catch(fail) }, keysActive);
  useAppCommand({ id: "openVault", title: "Open Vault file…", section: "Vaults", icon: FolderOpen, shortcut: SHORTCUTS.openVault, run: () => actions.showOpenVault() }, keysActive && !openVaultShown);
  useAppCommand({ id: "newVault", title: "New Vault…", section: "Vaults", icon: Plus, shortcut: SHORTCUTS.newVault, run: () => actions.showOpenVault("new") }, keysActive);
  useAppCommand({ id: "settings", title: "Settings", section: "Go to", icon: Settings, shortcut: SHORTCUTS.settings, enabled: anyUnlocked, run: () => actions.openSettings() }, keysActive);
  useAppCommand({ id: "plugins", title: "Plugins", section: "Go to", icon: Puzzle, enabled: anyUnlocked, run: () => actions.openPlugins() }, keysActive);
  useAppCommand({ id: "agentHosts", title: "Agent hosts", section: "Go to", icon: Bot, enabled: anyUnlocked, run: () => actions.openAgentHosts() }, keysActive);
  useQuitCommand(keysActive);
  useAppCommand({ id: "installPlugin", title: "Install a Plugin…", section: "App", icon: FileUp, enabled: anyUnlocked, run: () => void installFromFile(actions) }, keysActive);

  if (!snapshot) {
    return workspace.error ? <div className="p-6 text-danger">{errorMessage(workspace.error)}</div> : null;
  }
  return (
    <>
      {openVaultShown ? (
        <OpenVault
          snapshot={snapshot}
          mode={state.openVault ?? "unlock"}
          onModeChange={actions.showOpenVault}
          onDone={actions.closeOpenVault}
          onClose={anyUnlocked ? actions.closeOpenVault : undefined}
          onSettings={
            anyUnlocked
              ? () => {
                  actions.closeOpenVault();
                  actions.openSettings();
                }
              : undefined
          }
        />
      ) : (
        <Shell />
      )}
      {/* A request is answered first: the other overlays wait under it (the palette closes). */}
      {!openVaultShown && !hasPendingRequest(snapshot) && <Overlays />}
      <RequestDialogs snapshot={snapshot} />
      <Toaster />
    </>
  );
}
