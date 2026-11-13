// Global keyboard shortcuts. Combos are written as DESIGN.md writes them, in Linux/Windows
// notation: "Ctrl K", "Ctrl Shift P", "Escape", "/", "ArrowDown". "Mod" is Ctrl, or Cmd on a
// Mac. A shortcut doesn't fire while typing in a field unless it says `inFields`, and Escape
// and Ctrl/Cmd combos do by default, as users expect.
//
// The user can give keefast's shortcuts other keys in Settings (General Settings C); the backend
// keeps them in the app settings. `SHORTCUTS` always reads the keys in effect, and the hooks here
// re-render whoever binds or shows them when the user changes one.

import { useEffect, useRef, useSyncExternalStore } from "react";

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

export type ShortcutName = keyof typeof shortcuts;
export type Shortcuts = { readonly [Name in ShortcutName]: readonly string[] };

/** The app's global shortcuts before the user changes any. Ctrl K is the command palette, Ctrl
 * Shift P its alias. They live in `shortcuts.json` because the Rust side reads them too: the
 * Plugin host refuses a Plugin's command a shortcut keefast has (crates/plugins/src/shortcut.rs),
 * and Settings refuses the user keys another shortcut has (crates/app/src/shortcuts.rs). */
export const DEFAULT_SHORTCUTS: Shortcuts = shortcuts;

/** Shortcuts the user can't change, as the backend's `FIXED`: Escape and the arrows move through
 * every list, menu and dialog as well. */
export const FIXED_SHORTCUTS: readonly ShortcutName[] = ["close", "nextEntry", "previousEntry"];

let changed: Readonly<Partial<Record<string, readonly string[]>>> = {};
let version = 0;
const listeners = new Set<() => void>();

/** The app's shortcuts as the user has them: their keys where they changed them in Settings,
 * the defaults elsewhere. Read in render by a component that `useShortcuts` (or `useShortcut`,
 * `useAppCommand`), so it re-renders when they change. */
export const SHORTCUTS: Shortcuts = Object.defineProperties(
  {},
  Object.fromEntries(
    (Object.keys(DEFAULT_SHORTCUTS) as ShortcutName[]).map((name) => [
      name,
      { enumerable: true, get: () => changed[name] ?? DEFAULT_SHORTCUTS[name] },
    ]),
  ),
) as Shortcuts;

/** Takes the keys the user gave shortcuts, from the app settings (`AppSettings.shortcuts`). */
export function setChangedShortcuts(next: Readonly<Partial<Record<string, readonly string[]>>>) {
  if (JSON.stringify(next) === JSON.stringify(changed)) return;
  changed = next;
  version += 1;
  listeners.forEach((notify) => notify());
}

/** `SHORTCUTS`, re-rendering the component when the user changes a shortcut. */
export function useShortcuts(): Shortcuts {
  useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => listeners.delete(notify);
    },
    () => version,
  );
  return SHORTCUTS;
}

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "AltGraph", "Meta", "OS", "CapsLock", "Fn", "Dead", "Process", "Unidentified"]);

/** The combo a key press makes, written as `shortcuts.json` writes them ("Ctrl Shift K", "/",
 * "Delete"), to record a shortcut; `null` while only modifiers are down. Shift is left out for
 * symbols, which `matchesCombo` matches with or without it. */
export function comboFromEvent(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;
  const key = event.key === " " ? "Space" : event.key;
  const symbol = key.length === 1 && !/[a-z0-9]/i.test(key);
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.metaKey) parts.push("Meta");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey && !symbol) parts.push("Shift");
  parts.push(key.length === 1 ? key.toUpperCase() : key);
  return parts.join(" ");
}

/** The first of `holders` with a key that is `combo`, compared as `matchesCombo` would ("Mod"
 * being Ctrl here), or `undefined`. */
export function holderOf<Holder extends { keys: readonly string[] }>(combo: string, holders: readonly Holder[]): Holder | undefined {
  const same = (a: Combo, b: Combo) => a.key === b.key && a.ctrl === b.ctrl && a.shift === b.shift && a.alt === b.alt && a.meta === b.meta;
  const wanted = parseCombo(combo);
  return holders.find((holder) => holder.keys.some((keys) => same(parseCombo(keys), wanted)));
}

/** Runs `handler` on any of `combos`, window-wide, while mounted. */
export function useShortcut(
  combos: string | readonly string[],
  handler: (event: KeyboardEvent) => void,
  options: ShortcutOptions = {},
) {
  // Re-renders the caller with the keys in effect when the user changes a shortcut.
  useShortcuts();
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
