// What Vault settings says of a Pairing: when its browser last asked the Vault for anything,
// and when it was paired, as the prototype's Paired browsers list has them.

import type { PairingView } from "@/bindings";

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "Used 2 minutes ago", "Used yesterday", or "Not used yet"; `live` within the last hour, as
 * while the browser is open. */
export function usedText(lastUsed: number | null, now = Date.now() / 1000): { text: string; live: boolean } {
  if (lastUsed === null) return { text: "Not used yet", live: false };
  const ago = Math.max(0, now - lastUsed);
  const count = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"} ago`;
  if (ago < MINUTE) return { text: "Used just now", live: true };
  if (ago < HOUR) return { text: `Used ${count(Math.floor(ago / MINUTE), "minute")}`, live: true };
  if (ago < DAY) return { text: `Used ${count(Math.floor(ago / HOUR), "hour")}`, live: false };
  const days = Math.floor(ago / DAY);
  if (days < 2) return { text: "Used yesterday", live: false };
  if (days < 30) return { text: `Used ${count(days, "day")}`, live: false };
  if (days < 365) return { text: `Used ${count(Math.floor(days / 30), "month")}`, live: false };
  return { text: `Used ${count(Math.floor(days / 365), "year")}`, live: false };
}

/** "Paired 2 Aug 2026", or null when the Vault doesn't know. */
export function pairedText(created: number | null): string | null {
  if (created === null) return null;
  const date = new Date(created * 1000).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
  return `Paired ${date}`;
}

/** When any of the Pairings was last used, if one has been. */
export function lastUsed(pairings: readonly PairingView[]): number | null {
  return pairings.reduce<number | null>((latest, pairing) => (pairing.lastUsed !== null && (latest === null || pairing.lastUsed > latest) ? pairing.lastUsed : latest), null);
}
