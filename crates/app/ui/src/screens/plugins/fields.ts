// What the kit knows about Owned fields' kinds, for any Plugin's facet. Plugins have no clock, so
// whether a `monthYear` field (a card's expiry) has passed is said here, by the host, by the same
// rule the Card Plugin's `validate` warns by when the host passes it the month.

export type MonthYearState = "empty" | "invalid" | "expired" | "soon" | "ok";

/** Whether an `MM/YY` month has passed, is this month or the next ("soon"), or is later, by the
 * month `now` is in, in UTC. Good through its last day. */
export function monthYearState(text: string, now = new Date()): MonthYearState {
  const value = text.trim();
  if (!value) return "empty";
  const match = /^(\d{2})\/(\d{2})$/.exec(value);
  if (!match) return "invalid";
  const month = Number(match[1]);
  if (month < 1 || month > 12) return "invalid";
  const until = (2000 + Number(match[2])) * 12 + month - (now.getUTCFullYear() * 12 + now.getUTCMonth() + 1);
  if (until < 0) return "expired";
  return until <= 1 ? "soon" : "ok";
}
