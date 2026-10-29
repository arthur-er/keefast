// Global keyboard shortcuts. Combos are written as DESIGN.md writes them, in Linux/Windows
// notation: "Ctrl K", "Ctrl Shift P", "Escape", "/", "ArrowDown". "Mod" is Ctrl, or Cmd on a
// Mac. A shortcut doesn't fire while typing in a field unless it says `inFields`, and Escape
// and Ctrl/Cmd combos do by default, as users expect.

import { useEffect, useRef } from "react";

import shortcuts from "./shortcuts.json";

export interface Combo {
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/** Parses "Ctrl Shift P" (or "ctrl+shift+p") into a combo. */
export function parseCombo(text: string): Combo {
  const parts = text.split(/[\s+]+/).filter(Boolean);
  const combo: Combo = { key: "", ctrl: false, shift: false, alt: false, meta: false };
  for (const part of parts) {
    switch (part.toLowerCase()) {
      case "ctrl":
      case "control":
        combo.ctrl = true;
        break;
      case "mod":
        if (isMac) combo.meta = true;
        else combo.ctrl = true;
        break;
      case "shift":
        combo.shift = true;
        break;
      case "alt":
      case "option":
        combo.alt = true;
        break;
      case "cmd":
      case "meta":
      case "super":
        combo.meta = true;
        break;
      default:
        combo.key = part.length === 1 ? part.toLowerCase() : part.toLowerCase();
    }
  }
  return combo;
}

export function matchesCombo(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">, combo: Combo): boolean {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key.toLowerCase();
  // Shift changes the key for symbols ("/" vs "?"), so only letters and named keys check it.
  const symbol = key.length === 1 && !/[a-z0-9]/.test(key);
  return (
    key === combo.key &&
    event.ctrlKey === combo.ctrl &&
    event.altKey === combo.alt &&
    event.metaKey === combo.meta &&
    (symbol || event.shiftKey === combo.shift)
  );
}

/** Whether the event comes from somewhere the user types. */
export function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.closest("input, textarea, select, [contenteditable=''], [contenteditable='true']") !== null;
}

/** How a combo is shown in a <Kbd>: ["Ctrl", "K"]. */
export function comboLabel(text: string): string[] {
  const combo = parseCombo(text);
  const keys: string[] = [];
  if (combo.ctrl) keys.push("Ctrl");
  if (combo.meta) keys.push(isMac ? "⌘" : "Super");
  if (combo.alt) keys.push(isMac ? "⌥" : "Alt");
  if (combo.shift) keys.push("Shift");
  const names: Record<string, string> = { arrowup: "↑", arrowdown: "↓", arrowleft: "←", arrowright: "→", escape: "Esc", enter: "Enter", delete: "Del", home: "Home", end: "End", ",": "," };
  keys.push(names[combo.key] ?? (combo.key.length === 1 ? combo.key.toUpperCase() : combo.key));
  return keys;
}

export interface ShortcutOptions {
  /** Fire while typing in a field too. Defaults to true for Escape and Ctrl/Cmd combos. */
  inFields?: boolean;
  enabled?: boolean;
  /** Checked before the key is taken: when it says no, the key does what it would anyway (Ctrl C
   * copies the text the user selected rather than the password). */
  when?: (event: KeyboardEvent) => boolean;
}

/** The app's global shortcuts. Ctrl K is the command palette, Ctrl Shift P its alias. They live
 * in `shortcuts.json` because the Rust side reads them too: the Plugin host refuses a Plugin's
 * command a shortcut keefast has (crates/plugins/src/shortcut.rs). */
export const SHORTCUTS: { readonly [Name in ShortcutName]: readonly string[] } = shortcuts;

/** A shortcut's name in `shortcuts.json`: "lockAll", "palette". */
export type ShortcutName = keyof typeof shortcuts;

/** Runs `handler` on any of `combos`, window-wide, while mounted. */
export function useShortcut(
  combos: string | readonly string[],
  handler: (event: KeyboardEvent) => void,
  options: ShortcutOptions = {},
) {
  const latest = useRef(handler);
  latest.current = handler;
  const list = typeof combos === "string" ? [combos] : combos;
  const key = list.join("|");
  const { enabled = true, inFields } = options;
  const when = useRef(options.when);
  when.current = options.when;
  useEffect(() => {
    if (!enabled) return;
    const parsed = key.split("|").map(parseCombo);
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const combo = parsed.find((c) => matchesCombo(event, c));
      if (!combo) return;
      const fieldsToo = inFields ?? (combo.key === "escape" || combo.ctrl || combo.meta);
      if (!fieldsToo && isTyping(event.target)) return;
      // An open menu or listbox takes its own keys (arrows, Enter, Escape).
      if (event.target instanceof Element && event.target.closest('[role="menu"], [role="listbox"]')) return;
      if (when.current && !when.current(event)) return;
      event.preventDefault();
      latest.current(event);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [key, enabled, inFields]);
}
