// Dates and addresses as the screens show them (DESIGN.md: "Oct 7", "Apr 18, 2021").

const DAY = 86400;

/** A date as "Oct 7" this year, "Apr 18, 2021" in another. Times are seconds since 1970, UTC. */
export function formatDate(seconds: number, now = Date.now() / 1000): string {
  const date = new Date(seconds * 1000);
  const thisYear = new Date(now * 1000).getFullYear() === date.getFullYear();
  return date.toLocaleDateString("en", {
    month: "short",
    day: "numeric",
    ...(thisYear ? {} : { year: "numeric" }),
  });
}

/** Whole days from `now` to `seconds`, counted in calendar days of the local time zone. */
export function daysUntil(seconds: number, now = Date.now() / 1000): number {
  const midnight = (s: number) => {
    const date = new Date(s * 1000);
    return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 1000;
  };
  return Math.round((midnight(seconds) - midnight(now)) / DAY);
}

/** How an Entry's expiry reads: none, past, soon (within 30 days), or later. */
export type Expiry =
  | { kind: "never" }
  | { kind: "expired"; date: string }
  | { kind: "soon"; date: string; days: number }
  | { kind: "later"; date: string };

export function describeExpiry(expires: number | null, expired: boolean, now = Date.now() / 1000): Expiry {
  if (expires === null) return { kind: "never" };
  const date = formatDate(expires, now);
  if (expired || expires <= now) return { kind: "expired", date };
  const days = daysUntil(expires, now);
  return days <= 30 ? { kind: "soon", date, days } : { kind: "later", date };
}

/** The host of a URL ("github.com"), or the URL as it is when it has none. */
export function hostOf(url: string): string {
  const trimmed = url.trim();
  try {
    const host = new URL(trimmed).host;
    if (host) return host;
  } catch {
    // Not absolute: a bare host such as "github.com/login" reads as itself.
  }
  return trimmed.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "").replace(/\/.*$/, "");
}

/** A byte count as "1.2 KB" or "411 B". */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The parts of `text` around the first case-insensitive occurrence of `query`, to mark it. */
export function highlight(text: string, query: string): { before: string; match: string; after: string } | null {
  const needle = query.trim().toLowerCase();
  if (!needle) return null;
  const at = text.toLowerCase().indexOf(needle);
  if (at < 0) return null;
  return { before: text.slice(0, at), match: text.slice(at, at + needle.length), after: text.slice(at + needle.length) };
}
