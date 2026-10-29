//! The password generator, whose settings keefast and the Companion extension share, and how
//! strong a password is.

use keefast_core::{
    CharacterRecipe, LOOKALIKES, PassphraseRecipe, PasswordStrength, SYMBOLS, StrengthScore,
};
use keefast_workspace::{GeneratorMode, GeneratorSettings};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Core;
use crate::error::ApiResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GeneratorModeChoice {
    Characters,
    Passphrase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSettings {
    pub length: u32,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    pub exclude_lookalikes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PassphraseSettings {
    pub words: u32,
    pub separator: String,
}

/// How passwords are generated: the mode, and the settings of both modes, so switching mode and
/// back keeps them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorOptions {
    pub mode: GeneratorModeChoice,
    pub characters: CharacterSettings,
    pub passphrase: PassphraseSettings,
}

/// What the generator's controls may be set to.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorLimits {
    pub max_length: u32,
    pub max_words: u32,
    /// The symbols a password may use.
    pub symbols: String,
    /// The characters left out with `excludeLookalikes`.
    pub lookalikes: String,
}

impl From<&GeneratorSettings> for GeneratorOptions {
    fn from(settings: &GeneratorSettings) -> Self {
        let c = &settings.characters;
        Self {
            mode: match settings.mode {
                GeneratorMode::Characters => GeneratorModeChoice::Characters,
                GeneratorMode::Passphrase => GeneratorModeChoice::Passphrase,
            },
            characters: CharacterSettings {
                length: c.length,
                lowercase: c.lowercase,
                uppercase: c.uppercase,
                digits: c.digits,
                symbols: c.symbols,
                exclude_lookalikes: c.exclude_lookalikes,
            },
            passphrase: PassphraseSettings {
                words: settings.passphrase.words,
                separator: settings.passphrase.separator.clone(),
            },
        }
    }
}

impl From<GeneratorOptions> for GeneratorSettings {
    fn from(options: GeneratorOptions) -> Self {
        let c = options.characters;
        Self {
            mode: match options.mode {
                GeneratorModeChoice::Characters => GeneratorMode::Characters,
                GeneratorModeChoice::Passphrase => GeneratorMode::Passphrase,
            },
            characters: CharacterRecipe {
                length: c.length,
                lowercase: c.lowercase,
                uppercase: c.uppercase,
                digits: c.digits,
                symbols: c.symbols,
                exclude_lookalikes: c.exclude_lookalikes,
            },
            passphrase: PassphraseRecipe {
                words: options.passphrase.words,
                separator: options.passphrase.separator,
            },
        }
    }
}

/// What the generator's controls may be set to.
#[tauri::command]
#[specta::specta]
pub async fn generator_limits() -> ApiResult<GeneratorLimits> {
    Ok(GeneratorLimits {
        max_length: CharacterRecipe::MAX_LENGTH,
        max_words: PassphraseRecipe::MAX_WORDS,
        symbols: SYMBOLS.to_owned(),
        lookalikes: LOOKALIKES.to_owned(),
    })
}

/// Changes how passwords are generated, and remembers it in the app config.
#[tauri::command]
#[specta::specta]
pub async fn set_generator_settings(core: Core<'_>, settings: GeneratorOptions) -> ApiResult<()> {
    core.update(|workspace| workspace.set_generator_settings(settings.into()));
    Ok(())
}

/// How strong a password is, as the strength meter shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StrengthView {
    /// The estimated entropy, in bits.
    pub bits: u32,
    pub score: StrengthScoreChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StrengthScoreChoice {
    Weak,
    Fair,
    Good,
    Strong,
}

impl From<PasswordStrength> for StrengthView {
    fn from(strength: PasswordStrength) -> Self {
        Self {
            bits: strength.bits(),
            score: match strength.score() {
                StrengthScore::Weak => StrengthScoreChoice::Weak,
                StrengthScore::Fair => StrengthScoreChoice::Fair,
                StrengthScore::Good => StrengthScoreChoice::Good,
                StrengthScore::Strong => StrengthScoreChoice::Strong,
            },
        }
    }
}

/// A password the generator made, with how strong its recipe makes every password: the same
/// whichever comes out, so regenerating doesn't move the meter.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPassword {
    pub password: String,
    pub strength: StrengthView,
}

/// A new password or passphrase made with `settings`, or the saved settings when none are given.
/// An error says which setting is out of range.
#[tauri::command]
#[specta::specta]
pub async fn generate_password(
    core: Core<'_>,
    settings: Option<GeneratorOptions>,
) -> ApiResult<GeneratedPassword> {
    let recipe = match settings {
        Some(settings) => GeneratorSettings::from(settings).recipe(),
        None => core.read(|workspace| workspace.generator_settings().recipe()),
    };
    let password = recipe.generate()?;
    Ok(GeneratedPassword {
        password: password.expose_secret().to_owned(),
        strength: recipe
            .strength()
            .expect("a recipe that generates has a strength")
            .into(),
    })
}

/// How strong a password the user is typing is. A saved Entry's password needn't come here:
/// its detail has its strength.
#[tauri::command]
#[specta::specta]
pub async fn estimate_strength(password: String) -> ApiResult<StrengthView> {
    Ok(PasswordStrength::estimate(&password).into())
}
