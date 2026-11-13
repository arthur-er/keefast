//! The password generator: every password or passphrase it makes honours the recipe it was
//! asked for.

use std::collections::HashSet;

use keefast_core::{
    CharacterRecipe, GeneratorError, LOOKALIKES, PassphraseRecipe, PasswordRecipe,
    PasswordStrength, SYMBOLS, StrengthScore,
};
use secrecy::ExposeSecret;

type CharTest = fn(char) -> bool;

/// Enough samples that a class missing by chance would show up.
const SAMPLES: usize = 200;

fn generate(recipe: &PasswordRecipe) -> String {
    recipe
        .generate()
        .expect("the recipe is valid")
        .expose_secret()
        .to_owned()
}

fn characters(recipe: CharacterRecipe) -> PasswordRecipe {
    PasswordRecipe::Characters(recipe)
}

fn only(lowercase: bool, uppercase: bool, digits: bool, symbols: bool) -> CharacterRecipe {
    CharacterRecipe {
        length: 16,
        lowercase,
        uppercase,
        digits,
        symbols,
        exclude_lookalikes: false,
    }
}

#[test]
fn a_password_has_the_length_asked_for() {
    for length in [4, 12, 20, 64, CharacterRecipe::MAX_LENGTH] {
        let recipe = characters(CharacterRecipe {
            length,
            ..CharacterRecipe::default()
        });
        let password = generate(&recipe);
        assert_eq!(password.chars().count(), length as usize);
    }
}

#[test]
fn a_password_from_one_class_can_be_a_single_character() {
    let recipe = characters(CharacterRecipe {
        length: 1,
        ..only(false, false, true, false)
    });
    assert_eq!(generate(&recipe).chars().count(), 1);
}

#[test]
fn a_password_uses_only_the_character_classes_asked_for() {
    let cases: [(CharacterRecipe, CharTest); 4] = [
        (only(true, false, false, false), |c| c.is_ascii_lowercase()),
        (only(false, true, false, false), |c| c.is_ascii_uppercase()),
        (only(false, false, true, false), |c| c.is_ascii_digit()),
        (only(false, false, false, true), |c| SYMBOLS.contains(c)),
    ];
    for (recipe, allowed) in cases {
        for _ in 0..SAMPLES {
            let password = generate(&characters(recipe.clone()));
            assert!(password.chars().all(allowed), "{password:?}");
        }
    }
}

#[test]
fn a_password_has_at_least_one_character_of_every_class_asked_for() {
    let recipe = characters(CharacterRecipe {
        length: 4,
        ..CharacterRecipe::default()
    });
    for _ in 0..SAMPLES {
        let password = generate(&recipe);
        assert!(
            password.chars().any(|c| c.is_ascii_lowercase()),
            "{password:?}"
        );
        assert!(
            password.chars().any(|c| c.is_ascii_uppercase()),
            "{password:?}"
        );
        assert!(password.chars().any(|c| c.is_ascii_digit()), "{password:?}");
        assert!(
            password.chars().any(|c| SYMBOLS.contains(c)),
            "{password:?}"
        );
    }
}

#[test]
fn excluding_lookalikes_leaves_them_out() {
    let recipe = characters(CharacterRecipe {
        length: 64,
        exclude_lookalikes: true,
        ..CharacterRecipe::default()
    });
    for _ in 0..SAMPLES {
        let password = generate(&recipe);
        assert!(
            !password.chars().any(|c| LOOKALIKES.contains(c)),
            "{password:?}"
        );
    }
}

#[test]
fn without_excluding_lookalikes_they_can_appear() {
    let recipe = characters(CharacterRecipe {
        length: 64,
        ..CharacterRecipe::default()
    });
    let seen: HashSet<char> = (0..SAMPLES)
        .flat_map(|_| generate(&recipe).chars().collect::<Vec<_>>())
        .collect();
    assert!(LOOKALIKES.chars().all(|c| seen.contains(&c)));
}

#[test]
fn every_allowed_character_turns_up() {
    let recipe = characters(only(false, false, true, false));
    let seen: HashSet<char> = (0..SAMPLES)
        .flat_map(|_| generate(&recipe).chars().collect::<Vec<_>>())
        .collect();
    assert_eq!(seen.len(), 10);
}

#[test]
fn passwords_differ_every_time() {
    let recipe = PasswordRecipe::default();
    let passwords: HashSet<String> = (0..SAMPLES).map(|_| generate(&recipe)).collect();
    assert_eq!(passwords.len(), SAMPLES);
}

#[test]
fn a_password_needs_a_character_class() {
    let recipe = characters(only(false, false, false, false));
    assert_eq!(
        recipe.generate().err(),
        Some(GeneratorError::NoCharacterClasses)
    );
}

#[test]
fn a_password_must_be_long_enough_for_every_class_asked_for() {
    let recipe = characters(CharacterRecipe {
        length: 3,
        ..CharacterRecipe::default()
    });
    assert_eq!(
        recipe.generate().err(),
        Some(GeneratorError::LengthOutOfRange {
            min: 4,
            max: CharacterRecipe::MAX_LENGTH
        })
    );
}

#[test]
fn a_password_cannot_be_longer_than_the_maximum() {
    let recipe = characters(CharacterRecipe {
        length: CharacterRecipe::MAX_LENGTH + 1,
        ..CharacterRecipe::default()
    });
    assert!(matches!(
        recipe.generate(),
        Err(GeneratorError::LengthOutOfRange { .. })
    ));
}

#[test]
fn a_passphrase_has_the_word_count_and_separator_asked_for() {
    for (words, separator) in [
        (1, "-"),
        (4, " "),
        (6, "-"),
        (10, "::"),
        (PassphraseRecipe::MAX_WORDS, "."),
    ] {
        let recipe = PasswordRecipe::Passphrase(PassphraseRecipe {
            words,
            separator: separator.to_owned(),
        });
        let passphrase = generate(&recipe);
        let parts: Vec<&str> = passphrase.split(separator).collect();
        assert_eq!(parts.len(), words as usize, "{passphrase:?}");
        assert!(
            parts
                .iter()
                .all(|word| !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase())),
            "{passphrase:?}"
        );
    }
}

#[test]
fn a_passphrase_can_run_its_words_together() {
    let recipe = PasswordRecipe::Passphrase(PassphraseRecipe {
        words: 5,
        separator: String::new(),
    });
    let passphrase = generate(&recipe);
    assert!(passphrase.chars().all(|c| c.is_ascii_lowercase()));
    assert!(passphrase.len() >= 5 * 3);
}

#[test]
fn passphrase_words_come_from_a_large_word_list() {
    let recipe = PasswordRecipe::Passphrase(PassphraseRecipe {
        words: 1,
        separator: " ".to_owned(),
    });
    let words: HashSet<String> = (0..SAMPLES).map(|_| generate(&recipe)).collect();
    // Drawn from thousands of words, 200 picks barely repeat.
    assert!(words.len() > SAMPLES * 9 / 10, "{} distinct", words.len());
}

#[test]
fn a_passphrase_needs_a_word_count_in_range() {
    for words in [0, PassphraseRecipe::MAX_WORDS + 1] {
        let recipe = PasswordRecipe::Passphrase(PassphraseRecipe {
            words,
            separator: "-".to_owned(),
        });
        assert_eq!(
            recipe.generate().err(),
            Some(GeneratorError::WordCountOutOfRange {
                min: 1,
                max: PassphraseRecipe::MAX_WORDS
            })
        );
    }
}

#[test]
fn the_default_recipes_are_strong() {
    let password = generate(&PasswordRecipe::default());
    assert!(password.chars().count() >= 20);
    let passphrase = PasswordRecipe::Passphrase(PassphraseRecipe::default());
    assert!(PassphraseRecipe::default().words >= 6);
    assert!(!generate(&passphrase).is_empty());
}

#[test]
fn a_recipe_says_how_strong_its_passwords_are() {
    let strength = |recipe: &PasswordRecipe| recipe.strength().map(|strength| strength.bits());
    // 20 characters from all 94: 20 × log2(94).
    assert_eq!(strength(&PasswordRecipe::default()), Some(131));
    // 8 digits: 8 × log2(10).
    let digits = characters(CharacterRecipe {
        length: 8,
        ..only(false, false, true, false)
    });
    assert_eq!(strength(&digits), Some(27));
    assert_eq!(digits.strength().unwrap().score(), StrengthScore::Weak);
    // 6 words from the 7,772 of the word list: 6 × log2(7772).
    let passphrase = PasswordRecipe::Passphrase(PassphraseRecipe::default());
    assert_eq!(strength(&passphrase), Some(78));
    assert_eq!(passphrase.strength().unwrap().score(), StrengthScore::Good);
    // A recipe that can't make a password has no strength.
    assert_eq!(
        strength(&characters(only(false, false, false, false))),
        None
    );
}

#[test]
fn a_generated_passphrase_is_estimated_as_its_recipe_says() {
    let recipe = PasswordRecipe::Passphrase(PassphraseRecipe {
        words: 5,
        separator: ".".to_owned(),
    });
    for _ in 0..20 {
        assert_eq!(
            PasswordStrength::estimate(&generate(&recipe)),
            recipe.strength().unwrap()
        );
    }
}
