//! Fuzzy matching, the one matcher both the Entry search and the command palette use: how a word
//! the user typed matches a text — as typed, as the starts of the text's words ("cpass" for
//! "Copy Password"), or with a small typo ("gihtub" for "GitHub") — and which of the text's
//! characters it matched, to highlight them.

/// A word of a query, lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    chars: Vec<char>,
    /// How many typos it may have and still match.
    typos: usize,
}

/// How a word matched a text, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Found {
    /// Only allowing for a typo, in a word of the text or the start of one.
    Typo,
    /// As runs at the starts of the text's words, in order: "cpass" in "Copy Password".
    WordStarts,
    /// Somewhere in the middle of a word.
    Inside,
    /// At the start of one of the text's words.
    WordStart,
    /// At the start of the text.
    Start,
    /// As the whole text.
    Whole,
}

/// How a word matched a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WordMatch {
    pub found: Found,
    /// The byte offsets in the text of the characters matched, in order.
    pub positions: Vec<usize>,
}

/// The words of `query`, split at whitespace.
pub(crate) fn words(query: &str) -> Vec<Word> {
    query.split_whitespace().map(Word::new).collect()
}

impl Word {
    fn new(word: &str) -> Self {
        let chars: Vec<char> = word.chars().map(lowercase).collect();
        let typos = match chars.len() {
            0..4 => 0,
            4..8 => 1,
            _ => 2,
        };
        Self { chars, typos }
    }

    /// How the word matches `text`, ignoring case, or `None` if it doesn't. A match as typed
    /// beats one at word starts, which beats one allowing for a typo.
    pub(crate) fn find(&self, text: &str) -> Option<WordMatch> {
        let text = Text::new(text);
        if self.chars.is_empty() || text.chars.is_empty() {
            return None;
        }
        self.as_typed(&text)
            .or_else(|| self.at_word_starts(&text))
            .or_else(|| self.with_a_typo(&text))
    }

    /// The best place the word is in the text as typed.
    fn as_typed(&self, text: &Text) -> Option<WordMatch> {
        let len = self.chars.len();
        let mut best: Option<(Found, usize)> = None;
        for at in 0..=text.chars.len().checked_sub(len)? {
            if !text.chars[at..at + len]
                .iter()
                .map(|ch| ch.lower)
                .eq(self.chars.iter().copied())
            {
                continue;
            }
            let found = match at {
                0 if len == text.chars.len() => Found::Whole,
                0 => Found::Start,
                _ if text.chars[at].word_start => Found::WordStart,
                _ => Found::Inside,
            };
            if best.is_none_or(|(kept, _)| found > kept) {
                best = Some((found, at));
            }
        }
        let (found, at) = best?;
        Some(WordMatch {
            found,
            positions: text.offsets(at..at + len),
        })
    }

    /// The word as runs of characters each starting one of the text's words, in order, with
    /// as few runs as can be: "cpass" as "C" and "Pass" in "Copy Password", "gh" in "GitHub".
    fn at_word_starts(&self, text: &Text) -> Option<WordMatch> {
        let (n, m) = (self.chars.len(), text.chars.len());
        if n > m {
            return None;
        }
        // runs[i][j]: the fewest runs matching the word up to its character i, with that
        // character at the text's character j; `from[i][j]` is where character i - 1 was then.
        let mut runs = vec![vec![None::<usize>; m]; n];
        let mut from = vec![vec![0; m]; n];
        for i in 0..n {
            // The fewest runs of character i - 1 at any text character before j, and where.
            let mut fewest: Option<(usize, usize)> = None;
            for j in 0..m {
                if i > 0
                    && j > 0
                    && let Some(before) = runs[i - 1][j - 1]
                    && fewest.is_none_or(|(kept, _)| before < kept)
                {
                    fewest = Some((before, j - 1));
                }
                let ch = &text.chars[j];
                if ch.lower != self.chars[i] {
                    continue;
                }
                // Right after character i - 1 continues its run.
                let continued = (i > 0 && j > 0)
                    .then(|| runs[i - 1][j - 1])
                    .flatten()
                    .map(|before| (before, j - 1));
                // A word start can start a new run, after character i - 1 anywhere before it.
                let started = match (i, ch.word_start) {
                    (_, false) => None,
                    (0, true) => Some((1, 0)),
                    (_, true) => fewest.map(|(before, at)| (before + 1, at)),
                };
                let best = match (continued, started) {
                    (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
                    (a, b) => a.or(b),
                };
                if let Some((count, at)) = best {
                    runs[i][j] = Some(count);
                    from[i][j] = at;
                }
            }
        }
        let mut j = (0..m).filter_map(|j| Some((runs[n - 1][j]?, j))).min()?.1;
        let mut positions = vec![0; n];
        for i in (0..n).rev() {
            positions[i] = text.chars[j].offset;
            j = from[i][j];
        }
        Some(WordMatch {
            found: Found::WordStarts,
            positions,
        })
    }

    /// A word of the text, or the start of one as if still being typed, within the word's
    /// typos of it.
    fn with_a_typo(&self, text: &Text) -> Option<WordMatch> {
        if self.typos == 0 {
            return None;
        }
        let mut at = 0;
        while at < text.chars.len() {
            if !text.chars[at].ch.is_alphanumeric() {
                at += 1;
                continue;
            }
            let end = (at..text.chars.len())
                .find(|&end| !text.chars[end].ch.is_alphanumeric())
                .unwrap_or(text.chars.len());
            let word: Vec<char> = text.chars[at..end].iter().map(|ch| ch.lower).collect();
            let start = word.len().min(self.chars.len());
            for len in [word.len(), start] {
                if self.within_typos(&word[..len]) {
                    return Some(WordMatch {
                        found: Found::Typo,
                        positions: text.offsets(at..at + len),
                    });
                }
            }
            at = end;
        }
        None
    }

    /// Whether `word` is at most this word's typos away from it, counting a letter added,
    /// dropped or changed, or two next to each other swapped, as one typo each.
    fn within_typos(&self, word: &[char]) -> bool {
        if word.len().abs_diff(self.chars.len()) > self.typos {
            return false;
        }
        edit_distance(&self.chars, word) <= self.typos
    }
}

/// A text being matched, a character at a time.
struct Text {
    chars: Vec<TextChar>,
}

struct TextChar {
    /// Its byte offset in the text.
    offset: usize,
    ch: char,
    lower: char,
    /// Whether it starts a word: the text's first character, one after a character that is not
    /// a letter or digit, or a hump of camelCase.
    word_start: bool,
}

impl Text {
    fn new(text: &str) -> Self {
        let mut previous: Option<char> = None;
        let chars = text
            .char_indices()
            .map(|(offset, ch)| {
                let word_start = previous.is_none_or(|previous| {
                    !previous.is_alphanumeric() || (previous.is_lowercase() && ch.is_uppercase())
                });
                previous = Some(ch);
                TextChar {
                    offset,
                    ch,
                    lower: lowercase(ch),
                    word_start,
                }
            })
            .collect();
        Self { chars }
    }

    fn offsets(&self, range: std::ops::Range<usize>) -> Vec<usize> {
        self.chars[range].iter().map(|ch| ch.offset).collect()
    }
}

/// The lower case of a character that has a one-character lower case, so a text's characters
/// and their lower cases line up.
fn lowercase(ch: char) -> char {
    let mut lower = ch.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) => lower,
        _ => ch,
    }
}

/// The optimal string alignment distance: the Levenshtein distance, with swapping two adjacent
/// characters counting as one edit.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let width = b.len() + 1;
    let mut rows = vec![0; (a.len() + 1) * width];
    let at = |i: usize, j: usize| i * width + j;
    for i in 0..=a.len() {
        rows[at(i, 0)] = i;
    }
    for j in 0..=b.len() {
        rows[at(0, j)] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[at(i - 1, j)] + 1)
                .min(rows[at(i, j - 1)] + 1)
                .min(rows[at(i - 1, j - 1)] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[at(i - 2, j - 2)] + 1);
            }
            rows[at(i, j)] = best;
        }
    }
    rows[at(a.len(), b.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(word: &str, text: &str) -> Option<WordMatch> {
        Word::new(word).find(text)
    }

    fn found(word: &str, text: &str) -> Option<Found> {
        find(word, text).map(|found| found.found)
    }

    fn positions(word: &str, text: &str) -> Vec<usize> {
        find(word, text)
            .unwrap_or_else(|| panic!("{word:?} should match {text:?}"))
            .positions
    }

    fn distance(a: &str, b: &str) -> usize {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        edit_distance(&chars(a), &chars(b))
    }

    #[test]
    fn counts_edits_and_swaps() {
        assert_eq!(distance("github", "github"), 0);
        assert_eq!(distance("gihtub", "github"), 1);
        assert_eq!(distance("gthub", "github"), 1);
        assert_eq!(distance("githubb", "github"), 1);
        assert_eq!(distance("gitlab", "github"), 2);
        assert_eq!(distance("", "abc"), 3);
    }

    #[test]
    fn splits_queries_into_words() {
        assert_eq!(
            words("  copy   pass "),
            vec![Word::new("copy"), Word::new("pass")]
        );
        assert!(words("   ").is_empty());
    }

    #[test]
    fn matches_as_typed_best_place_first_ignoring_case() {
        assert_eq!(found("mail", "Mail"), Some(Found::Whole));
        assert_eq!(found("MAIL", "Mailbox"), Some(Found::Start));
        assert_eq!(found("mail", "Work mail"), Some(Found::WordStart));
        assert_eq!(found("mail", "Gmail"), Some(Found::Inside));
        // The word start beats the earlier match inside a word.
        assert_eq!(found("mail", "Gmail mail"), Some(Found::WordStart));
        assert_eq!(positions("mail", "Gmail mail"), [6, 7, 8, 9]);
    }

    #[test]
    fn matches_runs_at_word_starts() {
        assert_eq!(found("cpass", "Copy Password"), Some(Found::WordStarts));
        assert_eq!(positions("cp", "Copy Password"), [0, 5]);
        assert_eq!(positions("cpa", "Copy Password"), [0, 5, 6]);
        assert_eq!(
            positions("copypass", "Copy Password"),
            [0, 1, 2, 3, 5, 6, 7, 8]
        );
        assert_eq!(positions("sav", "Save all Vaults"), [0, 1, 2]);
        assert_eq!(positions("sav", "Sort all Vaults"), [0, 5, 9]);
        // camelCase humps start words too.
        assert_eq!(positions("gh", "GitHub"), [0, 3]);
        assert_eq!(positions("cu", "CopyUsername"), [0, 4]);
    }

    #[test]
    fn prefers_fewer_runs() {
        // "Sa" and "Vau" rather than "S", "a" and "Vau".
        assert_eq!(positions("savau", "Save all Vaults"), [0, 1, 9, 10, 11]);
    }

    #[test]
    fn letters_scattered_inside_words_do_not_match() {
        assert_eq!(found("cp", "Accept"), None);
        assert_eq!(found("cp", "Copy username"), None);
        assert_eq!(found("bdv", "billing@rezende.dev"), None);
        assert_eq!(found("ypoc", "Copy Password"), None);
    }

    #[test]
    fn matches_with_small_typos_in_longer_words() {
        assert_eq!(found("gihtub", "GitHub"), Some(Found::Typo));
        assert_eq!(found("octoccat", "octocat"), Some(Found::Typo));
        // Half-typed, with a typo.
        assert_eq!(positions("dorpb", "www.dropbox.com"), [4, 5, 6, 7, 8]);
        // Too short to guess at.
        assert_eq!(found("bak", "Bank"), None);
        assert_eq!(found("gitlab", "GitHub"), None);
    }

    #[test]
    fn positions_are_byte_offsets() {
        assert_eq!(positions("er", "Créer un coffre"), [4, 5]);
        assert_eq!(positions("uc", "Créer un coffre"), [7, 10]);
    }

    #[test]
    fn nothing_matches_an_empty_text() {
        assert_eq!(found("a", ""), None);
    }
}
