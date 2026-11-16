// Quitting keefast from the window: the "Quit keefast" command and its `quit` shortcut (Ctrl Q).
// With Close to tray on, closing the window no longer quits (ADR 0013), so this is how to; it
// works with the setting off as well.

import { LogOut } from "lucide-react";

import { api, errorMessage } from "@/api";
import { toast } from "@/kit";
import { SHORTCUTS } from "@/lib/shortcuts";

import { useAppCommand } from "./commands";

/** Registers "Quit keefast", bound to its shortcut while `active`. The backend quits as the
 * Tray's Quit does, clearing what keefast copied on the way out. */
export function useQuitCommand(active: boolean) {
  useAppCommand(
    {
      id: "quit",
      title: "Quit keefast",
      section: "App",
      icon: LogOut,
      shortcut: SHORTCUTS.quit,
      run: () => void api.quit().catch((error) => toast(errorMessage(error), { icon: "error" })),
    },
    active,
  );
}
