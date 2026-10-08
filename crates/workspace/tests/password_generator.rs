//! The password generator as the app and the browser reach it: generating with the user's
//! settings, which the app config remembers between runs.

mod support;

use std::sync::Arc;

use keefast_core::{CharacterRecipe, GeneratorError, PassphraseRecipe, PasswordRecipe};
use keefast_workspace::{
    AppConfig, ConfigStore, FileConfigStore, GeneratorMode, GeneratorSettings, Workspace,
};
use secrecy::ExposeSecret;
use support::{FakeConfigStore, FakeStorage};

fn workspace_with(config: Arc<dyn ConfigStore>) -> Workspace {
    Workspace::new(Arc::new(FakeStorage::default())).with_config(config)
}

fn passphrase_settings() -> GeneratorSettings {
    GeneratorSettings {
        mode: GeneratorMode::Passphrase,
        passphrase: PassphraseRecipe {
            words: 4,
            separator: " ".to_owned(),
        },
        ..GeneratorSettings::default()
    }
}

#[test]
fn a_fresh_workspace_generates_strong_passwords() {
    let workspace = workspace_with(Arc::new(FakeConfigStore::default()));

    let password = workspace.generate_password().unwrap();

    assert_eq!(
        workspace.generator_settings().recipe(),
        PasswordRecipe::default()
    );
    assert_eq!(password.expose_secret().chars().count(), 20);
}

#[test]
fn the_generator_follows_the_chosen_mode() {
    let mut workspace = workspace_with(Arc::new(FakeConfigStore::default()));

    workspace.set_generator_settings(passphrase_settings());

    let passphrase = workspace.generate_password().unwrap();
    assert_eq!(passphrase.expose_secret().split(' ').count(), 4);
}

#[test]
fn switching_mode_keeps_the_other_modes_settings() {
    let mut workspace = workspace_with(Arc::new(FakeConfigStore::default()));
    let mut settings = passphrase_settings();
    settings.characters.length = 32;
    workspace.set_generator_settings(settings.clone());

    settings.mode = GeneratorMode::Characters;
    workspace.set_generator_settings(settings);

    assert_eq!(
        workspace.generate_password().unwrap().expose_secret().len(),
        32
    );
    assert_eq!(workspace.generator_settings().passphrase.words, 4);
}

#[test]
fn settings_that_cannot_make_a_password_say_why() {
    let mut workspace = workspace_with(Arc::new(FakeConfigStore::default()));

    workspace.set_generator_settings(GeneratorSettings {
        characters: CharacterRecipe {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            ..CharacterRecipe::default()
        },
        ..GeneratorSettings::default()
    });

    assert_eq!(
        workspace.generate_password().err(),
        Some(GeneratorError::NoCharacterClasses)
    );
}

#[test]
fn the_app_config_remembers_the_generator_settings() {
    let config = Arc::new(FakeConfigStore::default());
    let mut workspace = workspace_with(config.clone());

    workspace.set_generator_settings(passphrase_settings());

    assert_eq!(config.saved().unwrap().generator, passphrase_settings());
}

#[test]
fn generator_settings_survive_a_restart_through_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let store = || Arc::new(FileConfigStore::new(path.clone()));
    let settings = GeneratorSettings {
        mode: GeneratorMode::Passphrase,
        characters: CharacterRecipe {
            length: 31,
            symbols: false,
            exclude_lookalikes: true,
            ..CharacterRecipe::default()
        },
        passphrase: PassphraseRecipe {
            words: 8,
            separator: ".".to_owned(),
        },
    };

    let mut first_run = workspace_with(store());
    first_run.set_generator_settings(settings.clone());
    first_run.set_autosave(true);
    drop(first_run);

    let second_run = workspace_with(store());
    assert!(second_run.config_error().is_none());
    assert_eq!(second_run.generator_settings(), &settings);
    assert!(second_run.autosave());
}

#[test]
fn generator_settings_this_version_does_not_understand_fall_back_to_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
autosave = true

[generator]
mode = "diceware-v2"
length = "long"
"#,
    )
    .unwrap();

    let config = FileConfigStore::new(path).load().unwrap();

    assert!(config.autosave);
    assert_eq!(config.generator, GeneratorSettings::default());
}

#[test]
fn generator_settings_missing_from_the_config_file_are_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[generator]
mode = "passphrase"
words = 5
"#,
    )
    .unwrap();

    let config = FileConfigStore::new(path).load().unwrap();

    assert_eq!(
        config.generator,
        GeneratorSettings {
            mode: GeneratorMode::Passphrase,
            passphrase: PassphraseRecipe {
                words: 5,
                ..PassphraseRecipe::default()
            },
            ..GeneratorSettings::default()
        }
    );
    assert_eq!(
        AppConfig::default().generator.recipe(),
        PasswordRecipe::default()
    );
}
