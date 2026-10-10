// What the kit knows about Owned fields' kinds, for any Plugin's facet. Plugins have no clock, so
// whether a `monthYear` field (a card's expiry) has passed is said here, by the host.

/** How long before a month ends that it is "soon". */
const SOON_DAYS = 60;

export type MonthYearState = "empty" | "invalid" | "expired" | "soon" | "ok";

/** Whether an `MM/YY` month has passed, ends within 60 days, or is later. Good through its last day. */
export function monthYearState(text: string, now = new Date()): MonthYearState {
  const value = text.trim();
  if (!value) return "empty";
  const match = /^(\d{2})\/(\d{2})$/.exec(value);
  if (!match) return "invalid";
  const month = Number(match[1]);
  if (month < 1 || month > 12) return "invalid";
  const end = Date.UTC(2000 + Number(match[2]), month, 1);
  if (end <= now.getTime()) return "expired";
  return end - now.getTime() < SOON_DAYS * 86_400_000 ? "soon" : "ok";
}
