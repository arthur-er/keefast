//! Fuzzy matching: whether a typed query matches a name with characters left out, as in
//! "cpass" for "Copy Password", and how well, to rank the names that match.

/// How a query matched a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// Higher is a better match.
    pub score: i32,
    /// The byte offsets in the name of the characters the query matched, to highlight them.
    pub positions: Vec<usize>,
}

/// Matches `query` against `name`: every character of the query must appear in the name in
/// order, ignoring case and the query's whitespace. Matches at the start of words and runs of
/// adjacent characters score higher; gaps between them score lower. An empty query matches
/// every name with a score of 0.
pub fn fuzzy_match(query: &str, name: &str) -> Option<FuzzyMatch> {
    let query: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(lowercase)
        .collect();
    let mut previous = None;
    let name: Vec<NameChar> = name
        .char_indices()
        .map(|(offset, ch)| {
            let word_start = match previous {
                None => true,
                Some(previous) => {
                    !char::is_alphanumeric(previous)
                        || (previous.is_lowercase() && ch.is_uppercase())
                }
            };
            previous = Some(ch);
            NameChar {
                offset,
                ch: lowercase(ch),
                word_start,
            }
        })
        .collect();
    if query.len() > name.len() {
        return None;
    }
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }

    // best[i][j]: the best score of the query up to its character i, with that character
    // matched at name character j; from[i][j]: where the query's character i - 1 matched then.
    let mut best = vec![vec![None::<i32>; name.len()]; query.len()];
    let mut from = vec![vec![0; name.len()]; query.len()];
    for (i, &wanted) in query.iter().enumerate() {
        for j in i..name.len() {
            if name[j].ch != wanted {
                continue;
            }
            let score = MATCH + if name[j].word_start { WORD_START } else { 0 };
            if i == 0 {
                best[i][j] = Some(score - j.min(MAX_LEADING_GAP) as i32 * LEADING_GAP);
                continue;
            }
            for k in i - 1..j {
                let Some(before) = best[i - 1][k] else {
                    continue;
                };
                let gap = j - k - 1;
                let joined = match gap {
                    0 => CONSECUTIVE,
                    gap => -(gap as i32) * GAP,
                };
                let candidate = before + score + joined;
                if best[i][j].is_none_or(|current| candidate > current) {
                    best[i][j] = Some(candidate);
                    from[i][j] = k;
                }
            }
        }
    }

    let last = query.len() - 1;
    let (mut j, score) = best[last]
        .iter()
        .enumerate()
        .filter_map(|(j, score)| Some((j, (*score)?)))
        .fold(
            None,
            |found: Option<(usize, i32)>, (j, score)| match found {
                Some((_, kept)) if kept >= score => found,
                _ => Some((j, score)),
            },
        )?;
    let mut positions = vec![0; query.len()];
    for i in (0..query.len()).rev() {
        positions[i] = name[j].offset;
        j = from[i][j];
    }
    Some(FuzzyMatch { score, positions })
}

/// A matched character.
const MATCH: i32 = 16;
/// A matched character starting a word, or a hump of camelCase.
const WORD_START: i32 = 24;
/// A matched character right after the previous one.
const CONSECUTIVE: i32 = 20;
/// Each character skipped between two matched ones.
const GAP: i32 = 3;
/// Each character skipped before the first matched one, up to `MAX_LEADING_GAP`.
const LEADING_GAP: i32 = 1;
const MAX_LEADING_GAP: usize = 3;

struct NameChar {
    offset: usize,
    ch: char,
    word_start: bool,
}

/// The lower case of a character that has a one-character lower case.
fn lowercase(ch: char) -> char {
    let mut lower = ch.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) => lower,
        _ => ch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, name: &str) -> i32 {
        fuzzy_match(query, name)
            .unwrap_or_else(|| panic!("{query:?} should match {name:?}"))
            .score
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(
            fuzzy_match("", "Save All"),
            Some(FuzzyMatch {
                score: 0,
                positions: vec![]
            })
        );
        assert_eq!(fuzzy_match("  ", "Save All").map(|m| m.score), Some(0));
    }

    #[test]
    fn query_characters_must_appear_in_order() {
        assert!(fuzzy_match("xyz", "Copy Password").is_none());
        assert!(fuzzy_match("ypoc", "Copy Password").is_none());
        assert!(fuzzy_match("copy passwords", "Copy Password").is_none());
    }

    #[test]
    fn ignores_case_and_query_whitespace() {
        assert!(fuzzy_match("COPY", "Copy Password").is_some());
        assert!(fuzzy_match("copy pass", "Copy Password").is_some());
        assert!(fuzzy_match("copypass", "Copy Password").is_some());
    }

    #[test]
    fn reports_matched_positions() {
        assert_eq!(
            fuzzy_match("cp", "Copy Password").unwrap().positions,
            vec![0, 5]
        );
        assert_eq!(
            fuzzy_match("save", "Save All").unwrap().positions,
            vec![0, 1, 2, 3]
        );
    }

    #[test]
    fn positions_are_byte_offsets() {
        assert_eq!(
            fuzzy_match("ér", "Créer un coffre").unwrap().positions,
            vec![2, 5]
        );
    }

    #[test]
    fn word_starts_beat_the_middle_of_words() {
        assert!(score("cp", "Copy Password") > score("cp", "Accept"));
        assert!(score("oe", "Open Entry") > score("oe", "Shoe"));
    }

    #[test]
    fn picks_the_best_alignment_not_the_first() {
        // A greedy match would take the "p" in "Copy"; the "P" of "Password" is a word start.
        assert_eq!(
            fuzzy_match("cpa", "Copy Password").unwrap().positions,
            vec![0, 5, 6]
        );
    }

    #[test]
    fn adjacent_characters_beat_scattered_ones() {
        assert!(score("user", "Copy Username") > score("user", "Undo Selected Row"));
    }

    #[test]
    fn camel_case_humps_are_word_starts() {
        assert!(score("cu", "CopyUsername") > score("cu", "Accumulate"));
    }

    #[test]
    fn shorter_gaps_beat_longer_ones() {
        assert!(score("ae", "Gate") > score("ae", "Gaggle"));
    }
}
