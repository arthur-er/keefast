// Ranking for command palette A. The backend matches (`searchPalette`, with the Entry search's
// fuzzy matcher, so the palette and the Entry list match alike); this orders what it matched and
// lays the palette out.

import type { EntrySummary, PaletteEntry, PaletteMatch } from "@/bindings";

export interface Ranked<T> {
  item: T;
  score: number;
  /** The title's matched characters, to highlight. */
  positions: number[];
}

/** The items whose title matched, best first; equal ones keep their order. `byName` holds how
 * each title matched, `null` when it didn't. */
export function rankByTitle<T extends { title: string }>(items: readonly T[], byName: ReadonlyMap<string, PaletteMatch | null>): Ranked<T>[] {
  return items
    .map((item, index) => ({ item, index, found: byName.get(item.title) ?? null }))
    .filter((x) => x.found !== null)
    .sort((a, b) => b.found!.score - a.found!.score || a.index - b.index)
    .map(({ item, found }) => ({ item, score: found!.score, positions: found!.positions }));
}

/** The Entries the backend found, best first. */
export function foundEntries(entries: readonly PaletteEntry[]): Ranked<EntrySummary>[] {
  return entries.map(({ entry, found }) => ({ item: entry, score: found.score, positions: found.positions }));
}

/** Whether the best Entry is so clearly the one meant that its actions show right under it: its
 * title matched, better than any other Entry. */
export function isConfident(ranked: readonly Ranked<EntrySummary>[]): boolean {
  const [best, next] = ranked;
  if (!best || best.positions.length === 0) return false;
  return !next || best.score > next.score;
}

/** Splits `text` into runs matched or not, to draw the matched ones highlighted. `positions`
 * are indexes in UTF-16 code units. */
export function highlightRuns(text: string, positions: readonly number[]): { text: string; matched: boolean }[] {
  const matched = new Set(positions);
  const runs: { text: string; matched: boolean }[] = [];
  let offset = 0;
  for (const ch of text) {
    const isMatched = matched.has(offset);
    const last = runs[runs.length - 1];
    if (last && last.matched === isMatched) last.text += ch;
    else runs.push({ text: ch, matched: isMatched });
    offset += ch.length;
  }
  return runs;
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
