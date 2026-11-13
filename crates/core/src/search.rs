//! Finding Entries: a query fuzzily matched (see `fuzzy`) against the fields the user remembers
//! an Entry by, and the orders the Entry list sorts in.
//!
//! Passwords, protected custom fields and TOTP secrets are never searched: whether a query
//! finds an Entry would otherwise tell what they hold.

use std::cmp::Reverse;

use secrecy::ExposeSecret;

use crate::fuzzy::{self, Found, Word};
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
/// though not all the same one; an empty query matches every Entry. The command palette matches
/// commands with it too, as if they were Entries' titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntrySearch {
    words: Vec<Word>,
}

/// The fields searched, most telling first, with how much a match in each counts.
#[derive(Clone, Copy)]
enum Field {
    Title = 4,
    UsernameOrUrl = 3,
    Tag = 2,
    Other = 1,
}

/// How a name matched a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameMatch {
    /// Higher is better, on the same scale as [`EntrySearch::relevance`].
    pub score: u32,
    /// The byte offsets in the name of the characters matched, in order, to highlight them.
    pub positions: Vec<usize>,
}

impl EntrySearch {
    pub fn new(query: &str) -> Self {
        Self {
            words: fuzzy::words(query),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// How well the Entry matches, higher being better, or `None` if it doesn't. Every Entry
    /// matches an empty search equally.
    ///
    /// A word found as typed, or as the starts of a field's words, counts by the field it is in
    /// and then by where in it: the whole field, its start, a word's start, anywhere, the
    /// starts of words. A word found only allowing for a typo counts less than any of those.
    pub fn relevance(&self, entry: &Entry) -> Option<u32> {
        let fields = searched_fields(entry);
        let mut total = 0;
        for word in &self.words {
            total += fields
                .iter()
                .filter_map(|(field, text)| Some(score(*field, word.find(text)?.found)))
                .max()?;
        }
        Some(total)
    }

    /// How `name` matches, scored as an Entry with it as its title would be, so commands and
    /// Entries rank together; or `None` if a word of the query doesn't match it. Every name
    /// matches an empty search equally.
    pub fn match_name(&self, name: &str) -> Option<NameMatch> {
        let mut score_total = 0;
        let mut positions = Vec::new();
        for word in &self.words {
            let found = word.find(name)?;
            score_total += score(Field::Title, found.found);
            positions.extend(found.positions);
        }
        positions.sort_unstable();
        positions.dedup();
        Some(NameMatch {
            score: score_total,
            positions,
        })
    }

    /// The byte offsets of the characters of `text` the query's words match, in order, to
    /// highlight them; words that don't match it are left out.
    pub fn highlights(&self, text: &str) -> Vec<usize> {
        let mut positions: Vec<usize> = self
            .words
            .iter()
            .filter_map(|word| word.find(text))
            .flat_map(|found| found.positions)
            .collect();
        positions.sort_unstable();
        positions.dedup();
        positions
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

/// What a word found in a field scores. A word found as typed or at word starts scores above
/// every typo, which scores at most `Field::Title`; and by its field first, as the best `Found`
/// adds less than a field's step.
fn score(field: Field, found: Found) -> u32 {
    const FOUND: u32 = 10;
    const FIELD_STEP: u32 = 5;
    let weight = field as u32;
    match found {
        Found::Typo => weight,
        found => FOUND + weight * FIELD_STEP + found as u32 - Found::WordStarts as u32,
    }
}

/// The Entry's fields a search looks in. Protected custom fields and those holding TOTP
/// settings are searched by name only.
fn searched_fields(entry: &Entry) -> Vec<(Field, &str)> {
    let mut fields = vec![
        (Field::Title, entry.title()),
        (Field::UsernameOrUrl, entry.username()),
        (Field::UsernameOrUrl, entry.url()),
        (Field::Other, entry.notes()),
    ];
    for tag in entry.tags() {
        fields.push((Field::Tag, tag.as_str()));
    }
    for field in entry.custom_fields() {
        fields.push((Field::Other, field.name.as_str()));
        if !field.protected && !TOTP_FIELDS.contains(&field.name.as_str()) {
            fields.push((Field::Other, field.value.expose_secret()));
        }
    }
    fields.retain(|(_, text)| !text.is_empty());
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_score_as_titles() {
        let search = EntrySearch::new("copy pass");
        let found = search.match_name("Copy Password").unwrap();
        assert_eq!(found.positions, [0, 1, 2, 3, 5, 6, 7, 8]);
        assert_eq!(
            found.score,
            score(Field::Title, Found::Start) + score(Field::Title, Found::WordStart)
        );
        assert_eq!(search.match_name("Copy username"), None);
        assert_eq!(
            EntrySearch::new("").match_name("Save all Vaults"),
            Some(NameMatch {
                score: 0,
                positions: vec![]
            })
        );
    }

    #[test]
    fn highlights_the_words_a_text_matches() {
        let search = EntrySearch::new("github octocat");
        assert_eq!(search.highlights("GitHub"), [0, 1, 2, 3, 4, 5]);
        assert!(search.highlights("Bank").is_empty());
    }

    #[test]
    fn word_starts_score_after_matches_as_typed_but_before_typos_in_a_field() {
        let inside = score(Field::Title, Found::Inside);
        let word_starts = score(Field::Title, Found::WordStarts);
        assert!(word_starts < inside);
        assert!(word_starts > score(Field::UsernameOrUrl, Found::Whole));
        assert!(score(Field::Other, Found::WordStarts) > score(Field::Title, Found::Typo));
    }
}
