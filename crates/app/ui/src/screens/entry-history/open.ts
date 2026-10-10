// Opening Entry history B: the shell's detail mode (`showHistory`), and which version to show
// first when the user picked one (the History tab of the Entry detail, the command palette).

import { History } from "lucide-react";

import { SHORTCUTS } from "@/lib/shortcuts";
import { useAppCommand } from "@/shell/commands";
import type { ShellActions } from "@/shell/state";

let requested: number | null = null;

/** Shows the selected Entry's history, at the version with that index in its history if given,
 * else at its newest earlier version. */
export function openHistory(shell: Pick<ShellActions, "showHistory">, version?: number) {
  requested = version ?? null;
  shell.showHistory();
}

/** The version asked for by `openHistory`, once. */
export function takeRequestedVersion(): number | null {
  const version = requested;
  requested = null;
  return version;
}

/** Registers Show Entry history (Ctrl H) while an Entry's detail shows. */
export function useShowHistoryCommand(shell: Pick<ShellActions, "showHistory">, active: boolean) {
  useAppCommand(
    {
      id: "showHistory",
      title: "Show Entry history",
      section: "Entry",
      icon: History,
      shortcut: SHORTCUTS.history,
      run: () => openHistory(shell),
    },
    active,
  );
}
