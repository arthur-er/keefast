//! Finding Entries: a query matched against the fields the user remembers an Entry by, small
//! typos included, and the orders the Entry list sorts in.
//!
//! Passwords, protected custom fields and TOTP secrets are never searched: whether a query
//! finds an Entry would otherwise tell what they hold.

use std::cmp::Reverse;

use secrecy::ExposeSecret;

use crate::totp::TOTP_FIELDS;
use crate::{Entry, Timestamp};

/// How the Entry list is sorted. Text sorts A to Z ignoring case, dates newest first; Entries
/// without the value come last.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EntrySort {
    #[default]
    Title,
    Username,
    Url,
    Created,
    Modified,
}

impl EntrySort {
    pub const ALL: [EntrySort; 5] = [
        EntrySort::Title,
        EntrySort::Username,
        EntrySort::Url,
        EntrySort::Created,
        EntrySort::Modified,
    ];

    fn key(self, entry: &Entry) -> SortKey {
        let text = |text: &str| SortKey::Text(text.is_empty(), text.to_lowercase());
        let time = |time: Option<Timestamp>| SortKey::Time(time.is_none(), time.map(Reverse));
        match self {
            EntrySort::Title => text(entry.title()),
            EntrySort::Username => text(entry.username()),
            EntrySort::Url => text(entry.url()),
            EntrySort::Created => time(entry.created()),
            EntrySort::Modified => time(entry.last_modified()),
        }
    }
}

/// What an Entry sorts by; the `bool`s put Entries without the value last.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Text(bool, String),
    Time(bool, Option<Reverse<Timestamp>>),
}

/// A search through Entries. Every word of the query must match one of an Entry's fields,
/// though not all the same one; an empty query matches every Entry.
#[derive(Debug, Clone, Default)]
pub struct EntrySearch {
    terms: Vec<Term>,
}

#[derive(Debug, Clone)]
struct Term {
    text: String,
    chars: Vec<char>,
    /// How many typos it may have and still match.
    typos: usize,
}

/// The fields searched, most telling first, with how much a match in each counts.
#[derive(Clone, Copy)]
enum Field {
    Title = 4,
    UsernameOrUrl = 3,
    Tag = 2,
    Other = 1,
}

/// How well a word of the query matches a field, worst first.
#[derive(Clone, Copy)]
enum Match {
    Inside = 1,
    WordStart = 2,
    Start = 3,
    Whole = 4,
}

impl EntrySearch {
    pub fn new(query: &str) -> Self {
        let terms = query
            .split_whitespace()
            .map(|word| {
                let text = word.to_lowercase();
                let chars: Vec<char> = text.chars().collect();
                let typos = match chars.len() {
                    0..4 => 0,
                    4..8 => 1,
                    _ => 2,
                };
                Term { text, chars, typos }
            })
            .collect();
        Self { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// How well the Entry matches, higher being better, or `None` if it doesn't. Every Entry
    /// matches an empty search equally.
    ///
    /// A word found as typed counts by the field it is in and then by where in it: the whole
    /// field, its start, a word's start, anywhere. A word found only allowing for a typo counts
    /// less than any found as typed.
    pub fn relevance(&self, entry: &Entry) -> Option<u32> {
        if self.terms.is_empty() {
            return Some(0);
        }
        let fields = searched_fields(entry);
        let mut total = 0;
        for term in &self.terms {
            total += fields
                .iter()
                .filter_map(|(field, text)| term.score(*field, text))
                .max()?;
        }
        Some(total)
    }

    /// The items whose Entry matches, best match first and equally good ones in `sort`
    /// order. Items that tie on both keep the order they came in.
    pub fn rank<T>(
        &self,
        items: impl IntoIterator<Item = T>,
        entry: impl Fn(&T) -> &Entry,
        sort: EntrySort,
    ) -> Vec<T> {
        let mut ranked: Vec<(Reverse<u32>, SortKey, T)> = items
            .into_iter()
            .filter_map(|item| {
                let entry = entry(&item);
                let relevance = self.relevance(entry)?;
                Some((Reverse(relevance), sort.key(entry), item))
            })
            .collect();
        ranked.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        ranked.into_iter().map(|(_, _, item)| item).collect()
    }
}

/// The Entry's fields a search looks in, lowercased. Protected custom fields and those holding
/// TOTP settings are searched by name only.
fn searched_fields(entry: &Entry) -> Vec<(Field, String)> {
    let mut fields = vec![
        (Field::Title, entry.title().to_lowercase()),
        (Field::UsernameOrUrl, entry.username().to_lowercase()),
        (Field::UsernameOrUrl, entry.url().to_lowercase()),
        (Field::Other, entry.notes().to_lowercase()),
    ];
    for tag in entry.tags() {
        fields.push((Field::Tag, tag.to_lowercase()));
    }
    for field in entry.custom_fields() {
        fields.push((Field::Other, field.name.to_lowercase()));
        if !field.protected && !TOTP_FIELDS.contains(&field.name.as_str()) {
            fields.push((Field::Other, field.value.expose_secret().to_lowercase()));
        }
    }
    fields.retain(|(_, text)| !text.is_empty());
    fields
}

impl Term {
    fn score(&self, field: Field, text: &str) -> Option<u32> {
        let weight = field as u32;
        if let Some(found) = self.find(text) {
            // Above every typo, which scores at most `Field::Title`.
            return Some(10 + weight * 5 + found as u32);
        }
        self.is_typo_of_a_word_in(text).then_some(weight)
    }

    fn find(&self, text: &str) -> Option<Match> {
        if text == self.text {
            return Some(Match::Whole);
        }
        if text.starts_with(&self.text) {
            return Some(Match::Start);
        }
        let mut found = None;
        for (index, _) in text.match_indices(&self.text) {
            let after_word = text[..index]
                .chars()
                .next_back()
                .is_some_and(|before| !before.is_alphanumeric());
            if after_word {
                return Some(Match::WordStart);
            }
            found = Some(Match::Inside);
        }
        found
    }

    /// Whether a word of `text`, or the start of one as if still being typed, is within this
    /// term's typos of it.
    fn is_typo_of_a_word_in(&self, text: &str) -> bool {
        if self.typos == 0 {
            return false;
        }
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .any(|word| {
                let word: Vec<char> = word.chars().collect();
                let start = &word[..word.len().min(self.chars.len())];
                self.within_typos(&word) || self.within_typos(start)
            })
    }

    /// Whether `word` is at most this term's typos away from it, counting a letter added,
    /// dropped or changed, or two next to each other swapped, as one typo each.
    fn within_typos(&self, word: &[char]) -> bool {
        if word.len().abs_diff(self.chars.len()) > self.typos {
            return false;
        }
        edit_distance(&self.chars, word) <= self.typos
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
}
