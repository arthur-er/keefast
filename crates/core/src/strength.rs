//! How strong a password is: an estimate of its entropy in bits, and a score from it.
//!
//! The estimate is the bits an attacker would need to guess, counted per character from the
//! kinds of character the password uses, with what is easy to guess counting next to nothing: a
//! character repeating the one before it or one step from it (`aaa`, `abc`, `321`), and the
//! fragments every guessing list starts with (`password`, `qwerty`, `123`). A passphrase of
//! words from the generator's word list counts its words, as the generator does. It is no
//! zxcvbn: a password made of a dictionary word with digits added still scores higher than it
//! deserves.

use crate::generator::WORD_LIST;

/// How strong a password is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PasswordStrength {
    bits: f64,
}

/// What [`PasswordStrength::bits`] amounts to, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrengthScore {
    /// Under 45 bits: guessed offline in moments.
    Weak,
    /// Under 64 bits.
    Fair,
    /// Under 90 bits.
    Good,
    Strong,
}

/// Fragments at the top of every guessing list, lowercase. A password that has one gains next
/// to nothing from its characters.
const COMMON: &[&str] = &[
    "password", "passw0rd", "qwerty", "azerty", "asdf", "zxcv", "letmein", "welcome", "admin",
    "login", "iloveyou", "monkey", "dragon", "master", "secret", "abc", "123", "111", "000",
];

/// Bits for a character that follows from the one before it, or is part of a common fragment.
const PREDICTABLE_BITS: f64 = 1.0;

impl PasswordStrength {
    /// The strength of `password`, as typed by a person or made by any generator.
    pub fn estimate(password: &str) -> Self {
        if let Some(words) = passphrase_words(password) {
            return Self::from_bits(words as f64 * word_bits());
        }
        let chars: Vec<char> = password.chars().collect();
        let common = common_positions(&chars);
        let per_char = pool_size(&chars).log2();
        let bits = chars
            .iter()
            .enumerate()
            .map(|(index, &char)| {
                let follows = index > 0 && (char as i64 - chars[index - 1] as i64).abs() <= 1;
                if follows || common[index] {
                    PREDICTABLE_BITS
                } else {
                    per_char
                }
            })
            .sum();
        Self::from_bits(bits)
    }

    pub(crate) fn from_bits(bits: f64) -> Self {
        Self {
            bits: bits.max(0.0),
        }
    }

    /// The estimated entropy, in whole bits.
    pub fn bits(&self) -> u32 {
        self.bits.round() as u32
    }

    pub fn score(&self) -> StrengthScore {
        match self.bits {
            bits if bits < 45.0 => StrengthScore::Weak,
            bits if bits < 64.0 => StrengthScore::Fair,
            bits if bits < 90.0 => StrengthScore::Good,
            _ => StrengthScore::Strong,
        }
    }
}

/// The bits one word of the generator's word list gives.
pub(crate) fn word_bits() -> f64 {
    (WORD_LIST.lines().count() as f64).log2()
}

/// How many words `password` has when it is words of the generator's word list, in any case,
/// joined by the same separator of up to three characters that aren't letters.
fn passphrase_words(password: &str) -> Option<usize> {
    let mut words = 0;
    let mut separator: Option<&str> = None;
    let mut rest = password;
    loop {
        let word_end = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let word = rest[..word_end].to_ascii_lowercase();
        if !WORD_LIST.lines().any(|known| known == word) {
            return None;
        }
        words += 1;
        rest = &rest[word_end..];
        if rest.is_empty() {
            return Some(words);
        }
        let between_end = rest
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let between = &rest[..between_end];
        if between.chars().count() > 3 || separator.is_some_and(|known| known != between) {
            return None;
        }
        separator = Some(between);
        rest = &rest[between_end..];
    }
}

/// How many characters each one of `chars` could have been, from the kinds it uses.
fn pool_size(chars: &[char]) -> f64 {
    let has = |test: fn(&char) -> bool| chars.iter().any(test);
    [
        (has(char::is_ascii_lowercase), 26.0),
        (has(char::is_ascii_uppercase), 26.0),
        (has(char::is_ascii_digit), 10.0),
        (has(|c| c.is_ascii_punctuation() || *c == ' '), 33.0),
        // Letters beyond ASCII: a generous guess at an alphabet's worth.
        (has(|c| !c.is_ascii()), 100.0),
    ]
    .into_iter()
    .filter(|(used, _)| *used)
    .map(|(_, size)| size)
    .sum::<f64>()
    .max(1.0)
}

/// Which of `chars` are inside a [`COMMON`] fragment, in any case.
fn common_positions(chars: &[char]) -> Vec<bool> {
    let lower: Vec<char> = chars.iter().map(char::to_ascii_lowercase).collect();
    let mut inside = vec![false; lower.len()];
    for fragment in COMMON {
        let fragment: Vec<char> = fragment.chars().collect();
        for start in 0..lower.len().saturating_sub(fragment.len() - 1) {
            if lower[start..start + fragment.len()] == fragment[..] {
                inside[start..start + fragment.len()].fill(true);
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(password: &str) -> u32 {
        PasswordStrength::estimate(password).bits()
    }

    fn score(password: &str) -> StrengthScore {
        PasswordStrength::estimate(password).score()
    }

    #[test]
    fn counts_bits_from_the_kinds_of_character_used() {
        // 12 lowercase letters with nothing predictable: 12 × log2(26).
        assert_eq!(bits("kxqmwrtzjvnh"), 56);
        // The same with every kind of ASCII character: 12 × log2(95).
        assert_eq!(bits("k7Q;w%Tz9vN~"), 79);
        assert_eq!(bits(""), 0);
    }

    #[test]
    fn repeats_and_runs_count_next_to_nothing() {
        assert_eq!(bits("aaaaaaaaaaaa"), bits("a") + 11);
        assert!(bits("abcdefghijkl") < 20);
        assert!(bits("98765432") < bits("97531864"));
    }

    #[test]
    fn common_fragments_count_next_to_nothing() {
        assert_eq!(score("Password123!"), StrengthScore::Weak);
        assert_eq!(score("qwerty"), StrengthScore::Weak);
        assert!(bits("letmein") < 10);
    }

    #[test]
    fn a_passphrase_of_list_words_counts_its_words() {
        let six = 6.0 * word_bits();
        assert_eq!(
            bits("anchor-willow-falcon-velvet-pebble-harbor"),
            six.round() as u32
        );
        assert_eq!(
            bits("Anchor.Willow.Falcon.Velvet.Pebble.Harbor"),
            six.round() as u32
        );
        assert_eq!(bits("tulip"), word_bits().round() as u32);
        // Mixed separators, or words not on the list, are just characters.
        assert_ne!(
            bits("anchor-willow_falcon"),
            (3.0 * word_bits()).round() as u32
        );
        assert_ne!(bits("anchor-willowz"), (2.0 * word_bits()).round() as u32);
    }

    #[test]
    fn scores_rise_with_the_bits() {
        assert_eq!(score("kxqmwrt"), StrengthScore::Weak);
        assert_eq!(score("kxqmwrtzjvnh"), StrengthScore::Fair);
        assert_eq!(score("k7Q;w%Tz9vN~"), StrengthScore::Good);
        assert_eq!(score("k7Q;w%Tz9vN~xP4!rL"), StrengthScore::Strong);
        assert!(StrengthScore::Weak < StrengthScore::Strong);
    }
}
