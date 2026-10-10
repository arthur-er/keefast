// The app's commands: what a key does, and what the command palette (Tauri UI 4/7) will list.
//
// A screen registers a command with `useAppCommand` while it is mounted; its shortcut is bound
// window-wide while the command is enabled, and `useAppCommands` lists every command registered,
// so the palette shows what the screens on display can do, with their keys. Register a command
// where its handler lives (the Entry list registers moving through Entries; the shell, locking),
// and give it the shortcut DESIGN.md §5 names, from `SHORTCUTS`.

import { useEffect, useRef, useSyncExternalStore } from "react";
import type { LucideIcon } from "lucide-react";

import { useShortcut } from "@/lib/shortcuts";

export interface AppCommand {
  /** Unique: a later registration with the same id replaces the earlier. */
  id: string;
  /** As the palette lists it: "Copy password", "Lock all Vaults". */
  title: string;
  /** The palette's section: "Entry", "Vaults", "Go to"… */
  section: string;
  icon?: LucideIcon;
  /** Keys that run it, as DESIGN.md writes them ("Ctrl C"). The first is the one shown. */
  shortcut?: readonly string[];
  /** Whether the keys work while typing in a field. Defaults as `useShortcut` does. */
  inFields?: boolean;
  /** Whether it can run now: a disabled command keeps its place in the palette, greyed. */
  enabled?: boolean;
  /** Left out of the palette: keys only. */
  hidden?: boolean;
  /** Checked when a key would run it: no leaves the key alone (see `useShortcut`). */
  when?: (event: KeyboardEvent) => boolean;
  run: () => void;
}

const registered = new Map<string, AppCommand>();
let snapshot: AppCommand[] = [];
const subscribers = new Set<() => void>();

function changed() {
  snapshot = [...registered.values()];
  subscribers.forEach((notify) => notify());
}

/** Registers `command` while mounted, and binds its shortcut while it is enabled and `active`
 * (the shell turns screens' keys off under a dialog or the palette). */
export function useAppCommand(command: AppCommand, active = true) {
  const latest = useRef(command);
  latest.current = command;
  const { id, title, section, icon, shortcut, enabled = true, hidden, inFields } = command;

  useEffect(() => {
    registered.set(id, { ...latest.current, run: () => latest.current.run() });
    changed();
    return () => {
      registered.delete(id);
      changed();
    };
    // The command's description changes rarely; re-register when it does.
  }, [id, title, section, icon, enabled, hidden, shortcut?.join("|")]);

  useShortcut(shortcut ?? [], () => latest.current.run(), {
    enabled: active && enabled && (shortcut?.length ?? 0) > 0,
    inFields,
    when: (event) => latest.current.when?.(event) ?? true,
  });
}

/** Every command registered now, for the command palette. */
export function useAppCommands(): readonly AppCommand[] {
  return useSyncExternalStore(
    (notify) => {
      subscribers.add(notify);
      return () => subscribers.delete(notify);
    },
    () => snapshot,
  );
}
