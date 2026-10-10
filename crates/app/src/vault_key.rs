//! The form that sets a Vault's master key and encryption: for a new Vault, written to a new
//! Vault file, or for an open Vault, saved to its Vault file straight away.

use std::path::{Path, PathBuf};

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, PathPromptOptions, SharedString,
    Task, Window, div, prelude::*,
};
use keefast_core::{Argon2, Cipher, Encryption, EncryptionError, Kdf, KeyFileError, SaveError};
use keefast_workspace::{
    ChangeKeyError, CreateVaultError, NewKeyError, NewVault, StorageError, UnlockSecrets, VaultId,
    VaultLocation, Workspace, write_new_key_file,
};
use secrecy::SecretString;

use crate::saving;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{Button, Segmented, Typography, check_mark};

pub enum VaultKeyEvent {
    /// The new Vault was created and added to the Workspace, or the open Vault's new settings
    /// are being saved.
    Done,
    Cancelled,
}

/// Which Vault the form is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Create,
    Change(VaultId),
}

/// The key derivation functions to pick from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KdfKind {
    Argon2id,
    Argon2d,
    AesKdf,
}

const MIB: u64 = 1024 * 1024;

/// The ciphers and key derivation functions to pick from, with their button ids and names.
const CIPHERS: [(&str, &str, Cipher); 3] = [
    ("cipher-aes", strings::CIPHER_AES, Cipher::Aes256),
    (
        "cipher-chacha20",
        strings::CIPHER_CHACHA20,
        Cipher::ChaCha20,
    ),
    ("cipher-twofish", strings::CIPHER_TWOFISH, Cipher::Twofish),
];
const KDFS: [(&str, &str, KdfKind); 3] = [
    ("kdf-argon2id", strings::KDF_ARGON2ID, KdfKind::Argon2id),
    ("kdf-argon2d", strings::KDF_ARGON2D, KdfKind::Argon2d),
    ("kdf-aes", strings::KDF_AES, KdfKind::AesKdf),
];

pub struct VaultKeyForm {
    workspace: Entity<Workspace>,
    purpose: Purpose,
    /// Where a new Vault file goes, and the name of its top Group.
    path: Entity<TextField>,
    name: Entity<TextField>,
    /// Whether the master key changes, for an open Vault, which starts off keeping its own; a
    /// new Vault always gets one.
    set_key: bool,
    password: Entity<TextField>,
    confirm: Entity<TextField>,
    key_file: Option<PathBuf>,
    cipher: Cipher,
    kdf: KdfKind,
    /// Argon2's memory in MiB, iterations and lanes, and AES-KDF's rounds.
    memory: Entity<TextField>,
    iterations: Entity<TextField>,
    parallelism: Entity<TextField>,
    rounds: Entity<TextField>,
    /// The settings the form started with, kept exactly where the user didn't change them.
    initial: Encryption,
    error: Option<SharedString>,
    /// The new Vault file being written.
    creating: Option<Task<()>>,
}

impl EventEmitter<VaultKeyEvent> for VaultKeyForm {}

impl Focusable for VaultKeyForm {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.purpose {
            Purpose::Create => self.path.focus_handle(cx),
            Purpose::Change(_) => self.password.focus_handle(cx),
        }
    }
}

impl VaultKeyForm {
    /// The form for a new Vault, with keefast's default encryption.
    pub fn new_vault(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self::new(workspace, Purpose::Create, Encryption::default(), cx)
    }

    /// The form for an open Vault's master key and encryption, starting from its own.
    pub fn for_vault(workspace: Entity<Workspace>, vault: VaultId, cx: &mut Context<Self>) -> Self {
        let current = workspace
            .read(cx)
            .vault(vault)
            .map(|open| open.vault().encryption())
            .unwrap_or_default();
        Self::new(workspace, Purpose::Change(vault), current, cx)
    }

    fn new(
        workspace: Entity<Workspace>,
        purpose: Purpose,
        initial: Encryption,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = |placeholder: &'static str, value: String, masked: bool| {
            let field = cx.new(|cx| {
                let mut field = TextField::new(placeholder, cx);
                field.set_text(&value, cx);
                if masked { field.masked() } else { field }
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        let argon2 = match initial.kdf {
            Kdf::Argon2id(argon2) | Kdf::Argon2d(argon2) => argon2,
            Kdf::AesKdf { .. } => Argon2::default(),
        };
        let rounds = match initial.kdf {
            Kdf::AesKdf { rounds } => rounds,
            _ => DEFAULT_AES_ROUNDS,
        };
        Self {
            path: field(strings::NEW_VAULT_PATH_PLACEHOLDER, String::new(), false),
            name: field(strings::NEW_VAULT_NAME_PLACEHOLDER, String::new(), false),
            set_key: purpose == Purpose::Create,
            password: field(strings::UNLOCK_PASSWORD_PLACEHOLDER, String::new(), true),
            confirm: field(strings::KEY_CONFIRM_PLACEHOLDER, String::new(), true),
            key_file: None,
            cipher: initial.cipher,
            kdf: match initial.kdf {
                Kdf::Argon2id(_) => KdfKind::Argon2id,
                Kdf::Argon2d(_) => KdfKind::Argon2d,
                Kdf::AesKdf { .. } => KdfKind::AesKdf,
            },
            memory: field(strings::KDF_MEMORY, mebibytes(argon2.memory), false),
            iterations: field(
                strings::KDF_ITERATIONS,
                argon2.iterations.to_string(),
                false,
            ),
            parallelism: field(
                strings::KDF_PARALLELISM,
                argon2.parallelism.to_string(),
                false,
            ),
            rounds: field(strings::KDF_ROUNDS, rounds.to_string(), false),
            initial,
            workspace,
            purpose,
            error: None,
            creating: None,
        }
    }

    fn on_field_event(
        &mut self,
        _: Entity<TextField>,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => self.submit(cx),
            TextFieldEvent::Cancel => self.cancel(cx),
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.creating.is_none() {
            cx.emit(VaultKeyEvent::Cancelled);
        }
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    /// The encryption the form describes. Argon2's memory, shown in MiB, stays exactly as it
    /// was unless its field changed.
    fn encryption(&self, cx: &App) -> Result<Encryption, &'static str> {
        let number = |field: &Entity<TextField>| {
            field
                .read(cx)
                .text()
                .trim()
                .parse::<u64>()
                .map_err(|_| strings::KDF_ERROR_NUMBER)
        };
        let argon2 = || -> Result<Argon2, &'static str> {
            let initial = match self.initial.kdf {
                Kdf::Argon2id(argon2) | Kdf::Argon2d(argon2) => Some(argon2.memory),
                Kdf::AesKdf { .. } => None,
            };
            let memory = match initial {
                Some(bytes) if self.memory.read(cx).text().trim() == mebibytes(bytes) => bytes,
                _ => number(&self.memory)?
                    .checked_mul(MIB)
                    .ok_or(strings::KDF_ERROR_MEMORY)?,
            };
            Ok(Argon2 {
                memory,
                iterations: number(&self.iterations)?,
                parallelism: u32::try_from(number(&self.parallelism)?)
                    .map_err(|_| strings::KDF_ERROR_PARALLELISM)?,
            })
        };
        let kdf = match self.kdf {
            KdfKind::Argon2id => Kdf::Argon2id(argon2()?),
            KdfKind::Argon2d => Kdf::Argon2d(argon2()?),
            KdfKind::AesKdf => Kdf::AesKdf {
                rounds: number(&self.rounds)?,
            },
        };
        let encryption = Encryption {
            cipher: self.cipher,
            kdf,
        };
        encryption.check().map_err(describe_encryption)?;
        Ok(encryption)
    }

    /// The master key the form describes, if it sets one.
    fn secrets(&self, cx: &App) -> Result<Option<UnlockSecrets>, &'static str> {
        if !self.set_key {
            return Ok(None);
        }
        let password = self.password.read(cx).text();
        if password != self.confirm.read(cx).text() {
            return Err(strings::KEY_ERROR_MISMATCH);
        }
        let password = SecretString::from(password);
        Ok(Some(UnlockSecrets::new(password, self.key_file.clone())))
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.creating.is_some() {
            return;
        }
        let checked = self
            .encryption(cx)
            .and_then(|encryption| Ok((encryption, self.secrets(cx)?)));
        let (encryption, secrets) = match checked {
            Ok(checked) => checked,
            Err(message) => return self.fail(message, cx),
        };
        match (self.purpose, secrets) {
            (Purpose::Change(vault), secrets) => self.change(vault, encryption, secrets, cx),
            // A new Vault always gets a master key.
            (Purpose::Create, secrets) => {
                let secrets = secrets.unwrap_or_else(|| UnlockSecrets::password(""));
                self.create(encryption, secrets, cx)
            }
        }
    }

    /// Writes the new Vault file off the UI thread, then adds and shows its Vault.
    fn create(&mut self, encryption: Encryption, secrets: UnlockSecrets, cx: &mut Context<Self>) {
        let Some(path) = expand_path(self.path.read(cx).text()) else {
            return self.fail(strings::NEW_VAULT_ERROR_NO_PATH, cx);
        };
        let new = NewVault {
            name: self.name.read(cx).text().to_owned(),
            secrets,
            encryption,
        };
        let pending = match self
            .workspace
            .read(cx)
            .begin_create(VaultLocation::Local(path), new)
        {
            Ok(pending) => pending,
            Err(error) => return self.fail(describe_create(&error), cx),
        };
        let created = cx
            .background_executor()
            .spawn(async move { pending.create() });
        self.error = None;
        self.creating = Some(cx.spawn(async move |this, cx| {
            let result = created.await;
            this.update(cx, |this, cx| {
                this.creating = None;
                match result {
                    Ok(created) => {
                        this.clear_passwords(cx);
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.add_unlocked(created);
                            cx.notify();
                        });
                        cx.emit(VaultKeyEvent::Done);
                    }
                    Err(error) => this.error = Some(describe_create(&error).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Changes the open Vault's master key and encryption, and saves them straight away.
    fn change(
        &mut self,
        vault: VaultId,
        encryption: Encryption,
        secrets: Option<UnlockSecrets>,
        cx: &mut Context<Self>,
    ) {
        let changed = self.workspace.update(cx, |workspace, cx| {
            cx.notify();
            if let Some(secrets) = &secrets {
                workspace.change_master_key(vault, secrets)?;
            }
            workspace.set_encryption(vault, &encryption)?;
            Ok::<_, ChangeKeyError>(())
        });
        match changed {
            Ok(()) => {
                self.clear_passwords(cx);
                saving::save(&self.workspace, vault, cx);
                cx.emit(VaultKeyEvent::Done);
            }
            Err(error) => self.fail(describe_change(&error), cx),
        }
    }

    /// Wipes the typed master password once it has been used (ADR 0004).
    fn clear_passwords(&mut self, cx: &mut Context<Self>) {
        for field in [&self.password, &self.confirm] {
            field.update(cx, |field, cx| field.set_text("", cx));
        }
    }

    fn choose_key_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(strings::UNLOCK_CHOOSE_KEY_FILE_PROMPT.into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                        this.key_file = Some(paths.swap_remove(0));
                        this.error = None;
                    }
                    Ok(Ok(_)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(strings::UNLOCK_ERROR_PICKER.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save a new key file, writes it there, and uses it.
    fn create_key_file(&mut self, cx: &mut Context<Self>) {
        let directory = self.suggested_directory(cx);
        let path = cx.prompt_for_new_path(&directory, Some(strings::NEW_KEY_FILE_NAME));
        cx.spawn(async move |this, cx| {
            let result = path.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(path))) => match write_new_key_file(&path) {
                        Ok(()) => {
                            this.key_file = Some(path);
                            this.error = None;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            this.error = Some(strings::NEW_KEY_FILE_ERROR_EXISTS.into())
                        }
                        Err(_) => this.error = Some(strings::NEW_KEY_FILE_ERROR_WRITE.into()),
                    },
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(strings::UNLOCK_ERROR_PICKER.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save the new Vault file.
    fn browse_path(&mut self, cx: &mut Context<Self>) {
        let directory = self.suggested_directory(cx);
        let path = cx.prompt_for_new_path(&directory, Some(strings::NEW_VAULT_FILE_NAME));
        cx.spawn(async move |this, cx| {
            let result = path.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(path))) => {
                        this.path
                            .update(cx, |field, cx| field.set_text(&path.to_string_lossy(), cx));
                        this.error = None;
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(strings::UNLOCK_ERROR_PICKER.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Where file dialogs start: by the new Vault file, else by the Vault's own, else home.
    fn suggested_directory(&self, cx: &App) -> PathBuf {
        let vault_file = match self.purpose {
            Purpose::Create => expand_path(self.path.read(cx).text()),
            Purpose::Change(vault) => match self.workspace.read(cx).vault(vault) {
                Some(open) => match open.location() {
                    VaultLocation::Local(path) => Some(path.clone()),
                    VaultLocation::Dropbox(_) => None,
                },
                None => None,
            },
        };
        vault_file
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn select_cipher(&mut self, cipher: Cipher, cx: &mut Context<Self>) {
        self.cipher = cipher;
        cx.notify();
    }

    fn select_kdf(&mut self, kdf: KdfKind, cx: &mut Context<Self>) {
        self.kdf = kdf;
        cx.notify();
    }

    fn toggle_set_key(&mut self, cx: &mut Context<Self>) {
        self.set_key = !self.set_key;
        self.error = None;
        cx.notify();
    }
}

/// AES-KDF's rounds when switching to it: about a second's worth, as KeePass suggests.
const DEFAULT_AES_ROUNDS: u64 = 2_000_000;

/// Argon2 memory in whole MiB, rounded up, as the form shows it.
fn mebibytes(bytes: u64) -> String {
    bytes.div_ceil(MIB).to_string()
}

/// A typed path made absolute, with `~` for the home directory. `None` when empty.
fn expand_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let path = match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(text),
    };
    Some(std::path::absolute(&path).unwrap_or(path))
}

fn describe_encryption(error: EncryptionError) -> &'static str {
    match error {
        EncryptionError::Iterations => strings::KDF_ERROR_ITERATIONS,
        EncryptionError::Parallelism => strings::KDF_ERROR_PARALLELISM,
        EncryptionError::Memory => strings::KDF_ERROR_MEMORY,
        EncryptionError::Rounds => strings::KDF_ERROR_ROUNDS,
    }
}

fn describe_key(error: &NewKeyError) -> &'static str {
    match error {
        NewKeyError::NoKey => strings::KEY_ERROR_NONE,
        NewKeyError::KeyFileNotFound(_) => strings::UNLOCK_ERROR_KEY_FILE_NOT_FOUND,
        NewKeyError::KeyFileUnreadable(_) => strings::UNLOCK_ERROR_KEY_FILE_READ,
        NewKeyError::KeyFile(KeyFileError::Corrupt(_)) => strings::UNLOCK_ERROR_KEY_FILE_CORRUPT,
        NewKeyError::KeyFile(KeyFileError::Unsupported(_)) => {
            strings::UNLOCK_ERROR_KEY_FILE_UNSUPPORTED
        }
    }
}

fn describe_create(error: &CreateVaultError) -> &'static str {
    match error {
        CreateVaultError::AlreadyOpen(_) => strings::NEW_VAULT_ERROR_OPEN,
        CreateVaultError::FileExists => strings::NEW_VAULT_ERROR_EXISTS,
        CreateVaultError::Key(error) => describe_key(error),
        CreateVaultError::Encryption(error) => describe_encryption(*error),
        CreateVaultError::Save(SaveError::Settings(_)) => strings::NEW_VAULT_ERROR_ENCRYPT,
        CreateVaultError::Storage(StorageError::Unavailable(_)) => {
            strings::UNLOCK_ERROR_NO_PROVIDER
        }
        CreateVaultError::Storage(_) => strings::NEW_VAULT_ERROR_WRITE,
    }
}

fn describe_change(error: &ChangeKeyError) -> &'static str {
    match error {
        ChangeKeyError::VaultNotOpen => strings::ERROR_NOT_FOUND,
        ChangeKeyError::Key(error) => describe_key(error),
        ChangeKeyError::Encryption(error) => describe_encryption(*error),
    }
}

impl Render for VaultKeyForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let label = |text: &'static str| div().text_secondary().text_color(theme.fg_1).child(text);
        let labelled = |text: &'static str, field: &Entity<TextField>| {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(label(text))
                .child(field.clone())
        };
        let is_new = self.purpose == Purpose::Create;
        let title: SharedString = match self.purpose {
            Purpose::Create => strings::NEW_VAULT_TITLE.into(),
            Purpose::Change(vault) => {
                let name = self
                    .workspace
                    .read(cx)
                    .vault(vault)
                    .map(|open| open.display_name())
                    .unwrap_or_default();
                strings::CHANGE_KEY_TITLE.replace("{vault}", &name).into()
            }
        };
        let key_file_label: SharedString = match &self.key_file {
            Some(path) => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
                .into(),
            None => strings::UNLOCK_NO_KEY_FILE.into(),
        };
        let argon2 = self.kdf != KdfKind::AesKdf;
        let working = self.creating.is_some();

        div()
            .id("vault-key")
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_dialog_title().child(title))
            .when(is_new, |this| {
                this.child(
                    div()
                        .flex()
                        .items_end()
                        .gap_2()
                        .child(labelled(strings::NEW_VAULT_PATH, &self.path))
                        .child(
                            Button::new("browse-vault-path", strings::NEW_VAULT_BROWSE)
                                .on_click(cx.listener(|this, _, _, cx| this.browse_path(cx))),
                        ),
                )
                .child(labelled(strings::NEW_VAULT_NAME, &self.name))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(strings::MASTER_KEY),
                    )
                    .when(!is_new, |this| {
                        this.child(
                            div()
                                .id("set-key")
                                .flex()
                                .gap_1()
                                .cursor_pointer()
                                .text_secondary()
                                .items_center()
                                .child(check_mark(self.set_key, theme))
                                .child(strings::CHANGE_KEY_TOGGLE)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_set_key(cx))),
                        )
                    }),
            )
            .when(self.set_key, |this| {
                this.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(labelled(strings::UNLOCK_PASSWORD_LABEL, &self.password))
                        .child(labelled(strings::KEY_CONFIRM_LABEL, &self.confirm)),
                )
                .child(label(strings::UNLOCK_KEY_FILE_LABEL))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_secondary()
                                .when(self.key_file.is_none(), |this| this.text_color(theme.fg_1))
                                .child(key_file_label),
                        )
                        .when(self.key_file.is_some(), |this| {
                            this.child(
                                Button::new("clear-new-key-file", strings::UNLOCK_CLEAR_KEY_FILE)
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.key_file = None;
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            Button::new("choose-new-key-file", strings::KEY_FILE_CHOOSE)
                                .on_click(cx.listener(|this, _, _, cx| this.choose_key_file(cx))),
                        )
                        .child(
                            Button::new("create-key-file", strings::KEY_FILE_CREATE)
                                .on_click(cx.listener(|this, _, _, cx| this.create_key_file(cx))),
                        ),
                )
                .child(
                    div()
                        .text_micro()
                        .text_color(theme.fg_1)
                        .child(strings::KEY_NOTE),
                )
            })
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(strings::ENCRYPTION),
            )
            .child(label(strings::CIPHER))
            .child(div().flex().child(CIPHERS.into_iter().fold(
                Segmented::new("cipher"),
                |choices, (_, name, cipher)| {
                    choices.option(
                        name,
                        self.cipher == cipher,
                        cx.listener(move |this, _, _, cx| this.select_cipher(cipher, cx)),
                    )
                },
            )))
            .child(label(strings::KDF))
            .child(div().flex().child(KDFS.into_iter().fold(
                Segmented::new("kdf"),
                |choices, (_, name, kdf)| {
                    choices.option(
                        name,
                        self.kdf == kdf,
                        cx.listener(move |this, _, _, cx| this.select_kdf(kdf, cx)),
                    )
                },
            )))
            .child(match argon2 {
                true => div()
                    .flex()
                    .gap_3()
                    .child(labelled(strings::KDF_MEMORY, &self.memory))
                    .child(labelled(strings::KDF_ITERATIONS, &self.iterations))
                    .child(labelled(strings::KDF_PARALLELISM, &self.parallelism)),
                false => div()
                    .flex()
                    .child(labelled(strings::KDF_ROUNDS, &self.rounds)),
            })
            .child(
                div()
                    .text_micro()
                    .text_color(theme.fg_1)
                    .child(strings::KDF_NOTE),
            )
            .when(working, |this| {
                this.child(
                    div()
                        .text_secondary()
                        .text_color(theme.fg_1)
                        .child(strings::NEW_VAULT_IN_PROGRESS),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_secondary().text_color(theme.danger).child(error))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("vault-key-cancel", strings::CANCEL)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                    )
                    .child(
                        Button::new(
                            "vault-key-submit",
                            match self.purpose {
                                Purpose::Create => strings::NEW_VAULT_CREATE,
                                Purpose::Change(_) => strings::CHANGE_KEY_APPLY,
                            },
                        )
                        .primary()
                        .disabled(working)
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{TestAppContext, VisualTestContext};
    use keefast_core::{CompositeKey, Vault};
    use keefast_workspace::LocalStorage;

    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/keepassxc-kdbx4.kdbx");
    const FIXTURE_PASSWORD: &str = "fixture-password";

    fn type_into(
        form: &Entity<VaultKeyForm>,
        field: impl Fn(&VaultKeyForm) -> &Entity<TextField>,
        text: &str,
        cx: &mut VisualTestContext,
    ) {
        form.update(cx, |form, cx| {
            field(form)
                .clone()
                .update(cx, |field, cx| field.set_text(text, cx))
        });
    }

    fn open_form(
        cx: &mut TestAppContext,
        make: impl FnOnce(Entity<Workspace>, &mut Context<VaultKeyForm>) -> VaultKeyForm + 'static,
        workspace: Workspace,
    ) -> (
        Entity<Workspace>,
        Entity<VaultKeyForm>,
        &mut VisualTestContext,
    ) {
        let workspace = cx.new(|_| workspace);
        let shared = workspace.clone();
        let (form, cx) = cx.add_window_view(move |_, cx| make(shared, cx));
        // Cheap key derivation, so the test doesn't wait on it.
        type_into(&form, |form| &form.memory, "1", cx);
        type_into(&form, |form| &form.iterations, "1", cx);
        (workspace, form, cx)
    }

    #[gpui::test]
    fn a_new_vault_needs_the_master_password_twice_then_is_created_and_opened(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Personal.kdbx");
        let (workspace, form, cx) = open_form(
            cx,
            VaultKeyForm::new_vault,
            Workspace::new(Arc::new(LocalStorage)),
        );
        type_into(&form, |form| &form.path, &path.to_string_lossy(), cx);
        type_into(&form, |form| &form.password, "secret one", cx);
        type_into(&form, |form| &form.confirm, "secret two", cx);

        form.update(cx, |form, cx| form.submit(cx));
        cx.run_until_parked();
        assert_eq!(
            form.read_with(cx, |form, _| form.error.clone()),
            Some(strings::KEY_ERROR_MISMATCH.into())
        );
        assert!(!path.exists());

        type_into(&form, |form| &form.confirm, "secret one", cx);
        form.update(cx, |form, cx| form.submit(cx));
        cx.run_until_parked();

        let vault = Vault::open(
            &std::fs::read(&path).unwrap(),
            &CompositeKey::password("secret one"),
        )
        .unwrap();
        assert_eq!(vault.root().name(), "Personal");
        assert_eq!(
            workspace.read_with(cx, |workspace, _| workspace.vaults().count()),
            1
        );
    }

    #[gpui::test]
    fn a_changed_master_key_and_kdf_are_saved_to_the_vault_file_straight_away(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Personal.kdbx");
        std::fs::write(&path, FIXTURE).unwrap();
        let mut workspace = Workspace::new(Arc::new(LocalStorage));
        let vault = workspace
            .open_vault(
                &VaultLocation::Local(path.clone()),
                &UnlockSecrets::password(FIXTURE_PASSWORD),
            )
            .unwrap();
        let (_, form, cx) = open_form(
            cx,
            move |workspace, cx| VaultKeyForm::for_vault(workspace, vault, cx),
            workspace,
        );
        form.update(cx, |form, cx| form.toggle_set_key(cx));
        type_into(&form, |form| &form.password, "new secret", cx);
        type_into(&form, |form| &form.confirm, "new secret", cx);
        // The fixture has AES-KDF.
        form.update(cx, |form, cx| form.select_kdf(KdfKind::Argon2id, cx));

        form.update(cx, |form, cx| form.submit(cx));
        cx.run_until_parked();

        let bytes = std::fs::read(&path).unwrap();
        let saved = Vault::open(&bytes, &CompositeKey::password("new secret")).unwrap();
        assert!(matches!(
            saved.encryption().kdf,
            Kdf::Argon2id(Argon2 {
                memory: MIB,
                iterations: 1,
                ..
            })
        ));
    }
}
