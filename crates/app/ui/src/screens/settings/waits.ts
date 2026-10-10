// The waits General Settings offers for the idle lock and for clearing the clipboard, as the
// GPUI settings offered them.

/** Seconds; `null` is never. */
export type Wait = number | null;

export const IDLE_LOCK_WAITS: readonly Wait[] = [60, 5 * 60, 15 * 60, 30 * 60, 60 * 60, 4 * 60 * 60, null];
export const CLIPBOARD_WAITS: readonly Wait[] = [10, 20, 30, 60, 2 * 60, 5 * 60, null];

/** A wait in its largest whole unit: "30 s", "5 min", "1 h" or "Never". */
export function describeWait(seconds: Wait): string {
  if (seconds === null) return "Never";
  if (seconds >= 3600 && seconds % 3600 === 0) return `${seconds / 3600} h`;
  if (seconds >= 60 && seconds % 60 === 0) return `${seconds / 60} min`;
  return `${seconds} s`;
}

/** The waits to offer, with `chosen` among them in its place even when it isn't one of
 * `offered`, as when the config file was edited by hand. */
export function waitsWith(offered: readonly Wait[], chosen: Wait): Wait[] {
  if (offered.includes(chosen)) return [...offered];
  const waits = [...offered];
  const at = waits.findIndex((wait) => wait === null || (chosen !== null && wait > chosen));
  waits.splice(at < 0 ? waits.length : at, 0, chosen);
  return waits;
}
