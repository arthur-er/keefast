//! The password generator: random passwords from character classes, and passphrases of random
//! words.
//!
//! Every pick is uniform and comes from the OS random number generator. Passphrase words come
//! from the EFF large word list (CC BY 3.0 US, eff.org/dice), without its four hyphenated
//! words so that `-` separates words unambiguously.

use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::strength::{PasswordStrength, word_bits};

/// The symbols a password may use: ASCII punctuation, as KeePassXC offers them.
pub const SYMBOLS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// Characters easily mistaken for one another, left out when asked to.
pub const LOOKALIKES: &str = "0O1Il|8B6G";

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";

pub(crate) const WORD_LIST: &str = include_str!("eff_large_wordlist.txt");

/// How to make a password: from character classes, or as a passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordRecipe {
    Characters(CharacterRecipe),
    Passphrase(PassphraseRecipe),
}

impl Default for PasswordRecipe {
    fn default() -> Self {
        Self::Characters(CharacterRecipe::default())
    }
}

/// A password of `length` characters drawn from the chosen classes, with at least one from
/// each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRecipe {
    pub length: u32,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    /// Leave out [`LOOKALIKES`].
    pub exclude_lookalikes: bool,
}

impl CharacterRecipe {
    pub const MAX_LENGTH: u32 = 256;

    /// The shortest password this recipe can make: one character from each chosen class.
    pub fn min_length(&self) -> u32 {
        [self.lowercase, self.uppercase, self.digits, self.symbols]
            .into_iter()
            .filter(|&chosen| chosen)
            .count()
            .max(1) as u32
    }
}

impl Default for CharacterRecipe {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            exclude_lookalikes: false,
        }
    }
}

/// A passphrase of `words` random words joined by `separator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseRecipe {
    pub words: u32,
    pub separator: String,
}

impl PassphraseRecipe {
    pub const MAX_WORDS: u32 = 40;
}

impl Default for PassphraseRecipe {
    fn default() -> Self {
        Self {
            words: 6,
            separator: "-".to_owned(),
        }
    }
}

/// Why a recipe can't make a password.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GeneratorError {
    #[error("choose at least one kind of character")]
    NoCharacterClasses,
    #[error("the length must be between {min} and {max}")]
    LengthOutOfRange { min: u32, max: u32 },
    #[error("the number of words must be between {min} and {max}")]
    WordCountOutOfRange { min: u32, max: u32 },
}

impl PasswordRecipe {
    /// A new random password or passphrase following this recipe.
    pub fn generate(&self) -> Result<SecretString, GeneratorError> {
        let password = match self {
            Self::Characters(recipe) => recipe.generate()?,
            Self::Passphrase(recipe) => recipe.generate()?,
        };
        Ok(SecretString::from(password.as_str()))
    }

    /// How strong every password this recipe makes is: the bits of its random picks, the same
    /// whichever password comes out. `None` when the recipe can't make one.
    pub fn strength(&self) -> Option<PasswordStrength> {
        let bits = match self {
            Self::Characters(recipe) => {
                let classes = recipe.classes();
                let valid = !classes.is_empty()
                    && (recipe.min_length()..=CharacterRecipe::MAX_LENGTH).contains(&recipe.length);
                let pool: usize = classes.iter().map(Vec::len).sum();
                valid.then(|| f64::from(recipe.length) * (pool as f64).log2())
            }
            Self::Passphrase(recipe) => (1..=PassphraseRecipe::MAX_WORDS)
                .contains(&recipe.words)
                .then(|| f64::from(recipe.words) * word_bits()),
        };
        bits.map(PasswordStrength::from_bits)
    }
}

impl CharacterRecipe {
    fn classes(&self) -> Vec<Vec<char>> {
        [
            (self.lowercase, LOWERCASE),
            (self.uppercase, UPPERCASE),
            (self.digits, DIGITS),
            (self.symbols, SYMBOLS),
        ]
        .into_iter()
        .filter(|(chosen, _)| *chosen)
        .map(|(_, class)| {
            class
                .chars()
                .filter(|c| !(self.exclude_lookalikes && LOOKALIKES.contains(*c)))
                .collect()
        })
        .collect()
    }

    fn generate(&self) -> Result<Zeroizing<String>, GeneratorError> {
        let classes = self.classes();
        if classes.is_empty() {
            return Err(GeneratorError::NoCharacterClasses);
        }
        let min = self.min_length();
        if !(min..=Self::MAX_LENGTH).contains(&self.length) {
            return Err(GeneratorError::LengthOutOfRange {
                min,
                max: Self::MAX_LENGTH,
            });
        }
        // One character from each class, the rest from all of them, then shuffled so the
        // guaranteed ones aren't always first.
        let all: Vec<char> = classes.concat();
        let mut chars: Zeroizing<Vec<char>> =
            Zeroizing::new(Vec::with_capacity(self.length as usize));
        chars.extend(classes.iter().map(|class| pick(class)));
        chars.extend((classes.len()..self.length as usize).map(|_| pick(&all)));
        for i in (1..chars.len()).rev() {
            chars.swap(i, random_below(i + 1));
        }
        let mut password = Zeroizing::new(String::with_capacity(chars.len()));
        password.extend(chars.iter());
        Ok(password)
    }
}

impl PassphraseRecipe {
    fn generate(&self) -> Result<Zeroizing<String>, GeneratorError> {
        if !(1..=Self::MAX_WORDS).contains(&self.words) {
            return Err(GeneratorError::WordCountOutOfRange {
                min: 1,
                max: Self::MAX_WORDS,
            });
        }
        let words: Vec<&str> = WORD_LIST.lines().collect();
        // Room for the longest passphrase up front, so growing it leaves no copies behind.
        let longest = words.iter().map(|word| word.len()).max().unwrap_or(0);
        let capacity = self.words as usize * (longest + self.separator.len());
        let mut passphrase = Zeroizing::new(String::with_capacity(capacity));
        for i in 0..self.words {
            if i > 0 {
                passphrase.push_str(&self.separator);
            }
            passphrase.push_str(pick(&words));
        }
        Ok(passphrase)
    }
}

fn pick<T: Copy>(choices: &[T]) -> T {
    choices[random_below(choices.len())]
}

/// A uniformly random number in `0..bound`, rejecting the draws that would bias it.
fn random_below(bound: usize) -> usize {
    let bound = u32::try_from(bound).expect("the generator picks from small sets");
    assert!(bound > 0);
    let zone = u32::MAX - (u32::MAX - bound + 1) % bound;
    loop {
        let draw = u32::from_le_bytes(crate::kdbx::random());
        if draw <= zone {
            return (draw % bound) as usize;
        }
    }
}
