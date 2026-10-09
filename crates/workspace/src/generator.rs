//! The user's password generator settings, which the app and the browser both generate with.

use keefast_core::{CharacterRecipe, PassphraseRecipe, PasswordRecipe};

/// Which kind of password the generator makes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GeneratorMode {
    #[default]
    Characters,
    Passphrase,
}

/// The generator's settings for both kinds of password, so switching mode and back keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratorSettings {
    pub mode: GeneratorMode,
    pub characters: CharacterRecipe,
    pub passphrase: PassphraseRecipe,
}

impl GeneratorSettings {
    /// The recipe for the chosen mode.
    pub fn recipe(&self) -> PasswordRecipe {
        match self.mode {
            GeneratorMode::Characters => PasswordRecipe::Characters(self.characters.clone()),
            GeneratorMode::Passphrase => PasswordRecipe::Passphrase(self.passphrase.clone()),
        }
    }
}
