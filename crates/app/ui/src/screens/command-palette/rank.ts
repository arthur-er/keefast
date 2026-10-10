// Ranking for command palette A: commands by their names, Entries by their titles (or, when sure
// enough, their usernames and sites), with keefast's fuzzy matching (`lib/fuzzy.ts`).

import type { EntrySummary } from "@/bindings";
import { fuzzyMatch } from "@/lib/fuzzy";
import { hostOf } from "@/lib/format";

export interface Ranked<T> {
  item: T;
  score: number;
  /** The title's matched characters, to highlight. */
  positions: number[];
}

/** The items whose title matches `query`, best first; equal ones keep their order. */
export function rankByTitle<T extends { title: string }>(query: string, items: readonly T[]): Ranked<T>[] {
  return items
    .map((item, index) => ({ item, index, found: fuzzyMatch(query, item.title) }))
    .filter((x) => x.found !== null)
    .sort((a, b) => b.found!.score - a.found!.score || a.index - b.index)
    .map(({ item, found }) => ({ item, score: found!.score, positions: found!.positions }));
}

/** How much a title match beats a username or site one. */
const TITLE_BONUS = 8;
const OTHER_PENALTY = 30;
/** Per query character, how well a username or site must match to be listed: a run of the
 * query at a word start, not letters scattered through an address. */
const CONFIDENT = 36;

/** The Entries matching `query`, best first: by title, or confidently by username or site. */
export function rankEntries(query: string, entries: readonly EntrySummary[]): Ranked<EntrySummary>[] {
  const typed = query.replace(/\s/g, "").length;
  if (typed === 0) return [];
  const ranked: (Ranked<EntrySummary> & { index: number })[] = [];
  entries.forEach((entry, index) => {
    const title = fuzzyMatch(query, entry.title);
    if (title) {
      ranked.push({ item: entry, score: title.score + TITLE_BONUS, positions: title.positions, index });
      return;
    }
    const other = [fuzzyMatch(query, entry.username), entry.url ? fuzzyMatch(query, hostOf(entry.url)) : null]
      .filter((found) => found !== null)
      .sort((a, b) => b.score - a.score)[0];
    if (other && other.score >= typed * CONFIDENT) ranked.push({ item: entry, score: other.score - OTHER_PENALTY, positions: [], index });
  });
  return ranked.sort((a, b) => b.score - a.score || a.index - b.index).map(({ item, score, positions }) => ({ item, score, positions }));
}

/** Whether the best Entry is so clearly the one meant that its actions show right under it. */
export function isConfident(ranked: readonly Ranked<EntrySummary>[]): boolean {
  const [best, next] = ranked;
  if (!best || best.positions.length === 0) return false;
  return !next || best.score - next.score > 12;
}

/** The palette's sections in order: the selected Entry's commands first, keys-only navigation
 * last, and sections the app doesn't know yet in between, by name. */
const ORDER = ["Entry", "History", "Editor", "Vaults", "View", "Go to", "App", "Navigation"];

export function orderSections(sections: Iterable<string>): string[] {
  const rank = (section: string) => {
    const at = ORDER.indexOf(section);
    if (at >= 0) return at;
    // Unknown sections go before "App" and "Navigation".
    return ORDER.indexOf("App") - 0.5;
  };
  return [...new Set(sections)].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}
