// Fuzzy matching, as `keefast_core::fuzzy_match` does it (crates/core/src/fuzzy.rs): every
// character of the query appears in the name in order, ignoring case and the query's spaces.
// Matches at the start of words and runs of adjacent characters score higher; gaps lower. The
// command palette ranks commands and Entry titles with it, and highlights the matched characters.

export interface FuzzyMatch {
  /** Higher is a better match. */
  score: number;
  /** The indexes (in UTF-16 code units, for slicing) of the name's matched characters. */
  positions: number[];
}

/** A matched character. */
const MATCH = 16;
/** A matched character starting a word, or a hump of camelCase. */
const WORD_START = 24;
/** A matched character right after the previous one. */
const CONSECUTIVE = 20;
/** Each character skipped between two matched ones. */
const GAP = 3;
/** Each character skipped before the first matched one, up to `MAX_LEADING_GAP`. */
const LEADING_GAP = 1;
const MAX_LEADING_GAP = 3;
/** No match there. */
const NONE = -Infinity;

const alphanumeric = (ch: string) => /[\p{L}\p{N}]/u.test(ch);
const lower = (ch: string) => ch !== ch.toUpperCase() && ch === ch.toLowerCase();
const upper = (ch: string) => ch !== ch.toLowerCase() && ch === ch.toUpperCase();
/** The lower case of a character that has a one-character lower case, as Rust's side does. */
const lowercase = (ch: string) => {
  const folded = ch.toLowerCase();
  return Array.from(folded).length === 1 ? folded : ch;
};

export function fuzzyMatch(query: string, name: string): FuzzyMatch | null {
  const wanted = Array.from(query)
    .filter((ch) => !/\s/u.test(ch))
    .map(lowercase);
  const chars: { offset: number; ch: string; wordStart: boolean }[] = [];
  let previous: string | null = null;
  let offset = 0;
  for (const ch of name) {
    const wordStart = previous === null || !alphanumeric(previous) || (lower(previous) && upper(ch));
    chars.push({ offset, ch: lowercase(ch), wordStart });
    previous = ch;
    offset += ch.length;
  }
  if (wanted.length > chars.length) return null;
  if (wanted.length === 0) return { score: 0, positions: [] };

  // best[i * n + j]: the best score of the query up to its character i, matched at name
  // character j (NONE when it can't be); from[i * n + j]: where character i - 1 matched then.
  const n = chars.length;
  const best = new Array<number>(wanted.length * n).fill(NONE);
  const from = new Array<number>(wanted.length * n).fill(0);
  const at = (array: number[], index: number) => array[index] ?? NONE;
  wanted.forEach((ch, i) => {
    for (let j = i; j < n; j++) {
      const named = chars[j]!;
      if (named.ch !== ch) continue;
      const score = MATCH + (named.wordStart ? WORD_START : 0);
      if (i === 0) {
        best[j] = score - Math.min(j, MAX_LEADING_GAP) * LEADING_GAP;
        continue;
      }
      for (let k = i - 1; k < j; k++) {
        const before = at(best, (i - 1) * n + k);
        if (before === NONE) continue;
        const gap = j - k - 1;
        const candidate = before + score + (gap === 0 ? CONSECUTIVE : -gap * GAP);
        if (candidate > at(best, i * n + j)) {
          best[i * n + j] = candidate;
          from[i * n + j] = k;
        }
      }
    }
  });

  const last = wanted.length - 1;
  let end = -1;
  let score = NONE;
  for (let j = 0; j < n; j++) {
    // The first of equal scores, as the Rust fold keeps it.
    const candidate = at(best, last * n + j);
    if (candidate > score) {
      score = candidate;
      end = j;
    }
  }
  if (score === NONE) return null;
  const positions = new Array<number>(wanted.length).fill(0);
  for (let i = last, j = end; i >= 0; i--) {
    positions[i] = chars[j]!.offset;
    j = at(from, i * n + j);
  }
  return { score, positions };
}

/** Splits `text` into runs matched or not, to draw the matched ones highlighted. */
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
