use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, EventEmitter, FocusHandle,
    Focusable, PathPromptOptions, SharedString, Task, Window, div, prelude::*, px, relative,
};
use keefast_core::{KeyFileError, OpenError};
use keefast_workspace::{
    EntryScope, OpenVaultError, StorageError, UnlockSecrets, VaultId, VaultLocation, View,
    Workspace,
};
use secrecy::SecretString;

use crate::actions::OpenSettings;
use crate::dropbox_panel::{self, DropboxPanel, DropboxPanelEvent};
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{Button, Kbd, Typography};
use crate::vault_key::{VaultKeyEvent, VaultKeyForm};

pub enum UnlockEvent {
    /// A Vault has been decrypted and added to the Workspace, or was open already. The
    /// Workspace shows it, or returns to where the user was in it if it was locked.
    Unlocked,
    /// The user went back to the Vaults already open.
    Cancelled,
}

/// The unlock dialog: pick a locked Vault or a recent Vault file, or choose one, type the
/// master password and/or choose a key file, unlock off the UI thread.
pub struct UnlockView {
    workspace: Entity<Workspace>,
    location: Option<VaultLocation>,
    password: Entity<TextField>,
    key_file: Option<PathBuf>,
    error: Option<UnlockError>,
    unlocking: Option<Task<()>>,
    /// Shown instead of the dialog while the user picks a Vault file in Dropbox.
    dropbox: Option<Entity<DropboxPanel>>,
    /// Shown instead of the dialog while the user creates a new Vault.
    new_vault: Option<Entity<VaultKeyForm>>,
}

/// Why the last unlock failed: what the user should know, plus the technical reason, if any.
#[derive(Clone)]
struct UnlockError {
    message: SharedString,
    detail: Option<SharedString>,
}

impl UnlockError {
    fn new(message: &'static str) -> Self {
        Self {
            message: message.into(),
            detail: None,
        }
    }
}

impl EventEmitter<UnlockEvent> for UnlockView {}

impl Focusable for UnlockView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.password.focus_handle(cx)
    }
}

impl UnlockView {
    /// Starts on `location`, or else on the most recent Vault file that isn't open.
    pub fn new(
        workspace: Entity<Workspace>,
        location: Option<VaultLocation>,
        key_file: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let password =
            cx.new(|cx| TextField::new(strings::UNLOCK_PASSWORD_PLACEHOLDER, cx).masked());
        cx.subscribe(&password, |this, _, event, cx| match event {
            TextFieldEvent::Submit => this.unlock(cx),
            TextFieldEvent::Cancel => this.cancel(cx),
        })
        .detach();
        let mut view = Self {
            workspace,
            location,
            password,
            key_file,
            error: None,
            unlocking: None,
            dropbox: None,
            new_vault: None,
        };
        if view.location.is_none() {
            view.location = view.most_recent_closed(cx);
        }
        view
    }

    /// Readies the dialog to open another Vault: no password or key file, and the most recent
    /// Vault file that isn't open yet.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.clear_password(cx);
        self.key_file = None;
        self.error = None;
        self.dropbox = None;
        self.new_vault = None;
        self.location = self.most_recent_closed(cx);
        cx.notify();
    }

    /// Shows the form for a new Vault instead of the dialog.
    pub fn new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.new(|cx| VaultKeyForm::new_vault(self.workspace.clone(), cx));
        cx.subscribe_in(&form, window, |this, _, event, window, cx| {
            this.new_vault = None;
            match event {
                VaultKeyEvent::Done => {
                    this.clear_password(cx);
                    cx.emit(UnlockEvent::Unlocked);
                }
                VaultKeyEvent::Cancelled => window.focus(&this.password.focus_handle(cx), cx),
            }
            cx.notify();
        })
        .detach();
        window.focus(&form.focus_handle(cx), cx);
        self.dropbox = None;
        self.new_vault = Some(form);
        cx.notify();
    }

    /// Readies the dialog to unlock a locked Vault.
    pub fn unlock_locked(&mut self, vault: VaultId, cx: &mut Context<Self>) {
        self.clear_password(cx);
        self.key_file = None;
        self.error = None;
        if let Some(locked) = self.workspace.read(cx).locked_vault(vault) {
            self.location = Some(locked.location().clone());
        }
        cx.notify();
    }

    fn most_recent_closed(&self, cx: &App) -> Option<VaultLocation> {
        self.workspace
            .read(cx)
            .closed_recent_vaults()
            .next()
            .cloned()
    }

    fn clear_password(&mut self, cx: &mut Context<Self>) {
        self.password.update(cx, |field, cx| field.set_text("", cx));
    }

    fn can_cancel(&self, cx: &App) -> bool {
        self.workspace.read(cx).vaults().next().is_some()
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.can_cancel(cx) && self.unlocking.is_none() {
            cx.emit(UnlockEvent::Cancelled);
        }
    }

    fn select(&mut self, location: VaultLocation, window: &mut Window, cx: &mut Context<Self>) {
        self.location = Some(location);
        self.error = None;
        window.focus(&self.password.focus_handle(cx), cx);
        cx.notify();
    }

    fn forget(&mut self, location: VaultLocation, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.forget_recent(&location);
            cx.notify();
        });
        if self.location.as_ref() == Some(&location) {
            self.location = None;
        }
        cx.notify();
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        self.pick_file(strings::UNLOCK_CHOOSE_FILE_PROMPT, cx, |this, path| {
            this.location = Some(VaultLocation::Local(path));
        });
    }

    fn choose_from_dropbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dropbox) = self.workspace.read(cx).dropbox().cloned() else {
            return;
        };
        let panel = cx.new(|cx| DropboxPanel::new(dropbox, cx));
        cx.subscribe_in(&panel, window, |this, _, event, window, cx| {
            if let DropboxPanelEvent::Picked(location) = event {
                this.location = Some(location.clone());
                this.error = None;
            }
            this.dropbox = None;
            window.focus(&this.password.focus_handle(cx), cx);
            cx.notify();
        })
        .detach();
        window.focus(&panel.focus_handle(cx), cx);
        self.dropbox = Some(panel);
        cx.notify();
    }

    fn choose_key_file(&mut self, cx: &mut Context<Self>) {
        self.pick_file(strings::UNLOCK_CHOOSE_KEY_FILE_PROMPT, cx, |this, path| {
            this.key_file = Some(path);
        });
    }

    fn clear_key_file(&mut self, cx: &mut Context<Self>) {
        self.key_file = None;
        self.error = None;
        cx.notify();
    }

    fn pick_file(
        &mut self,
        prompt: &'static str,
        cx: &mut Context<Self>,
        on_picked: impl FnOnce(&mut Self, PathBuf) + 'static,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this, cx| {
            let result = paths.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                        on_picked(this, paths.swap_remove(0));
                        this.error = None;
                    }
                    Ok(Ok(_)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(UnlockError::new(strings::UNLOCK_ERROR_PICKER)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn unlock(&mut self, cx: &mut Context<Self>) {
        let Some(location) = self.location.clone() else {
            return;
        };
        if self.unlocking.is_some() {
            return;
        }
        let secrets = UnlockSecrets::new(
            SecretString::from(self.password.read(cx).text()),
            self.key_file.clone(),
        );
        let pending = match self.workspace.read(cx).begin_open(location, secrets) {
            Ok(pending) => pending,
            Err(OpenVaultError::AlreadyOpen(vault)) => {
                self.clear_password(cx);
                self.workspace.update(cx, |workspace, _| {
                    workspace.show(View::from(EntryScope::Vault(vault)))
                });
                cx.emit(UnlockEvent::Unlocked);
                return;
            }
            Err(error) => {
                self.error = Some(describe(&error));
                cx.notify();
                return;
            }
        };
        let unlock = cx
            .background_executor()
            .spawn(async move { pending.unlock() });
        self.error = None;
        self.unlocking = Some(cx.spawn(async move |this, cx| {
            let result = unlock.await;
            this.update(cx, |this, cx| {
                this.unlocking = None;
                match result {
                    Ok(unlocked) => {
                        this.clear_password(cx);
                        this.workspace.update(cx, |workspace, cx| {
                            cx.notify();
                            workspace.add_unlocked(unlocked);
                        });
                        cx.emit(UnlockEvent::Unlocked);
                    }
                    Err(error) => this.error = Some(describe(&error)),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn render_recents(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let recents: Vec<VaultLocation> = self
            .workspace
            .read(cx)
            .closed_recent_vaults()
            .cloned()
            .collect();
        self.render_locations("recent", strings::RECENT_VAULTS, recents, true, theme, cx)
    }

    fn render_locked(&self, theme: Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let locked: Vec<VaultLocation> = self
            .workspace
            .read(cx)
            .locked_vaults()
            .map(|locked| locked.location().clone())
            .collect();
        self.render_locations("locked", strings::LOCKED_VAULTS, locked, false, theme, cx)
    }

    /// A list of Vault files to pick from under `title`, each with a button to forget it if
    /// `forgettable`.
    fn render_locations(
        &self,
        id: &'static str,
        title: &'static str,
        locations: Vec<VaultLocation>,
        forgettable: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if locations.is_empty() {
            return None;
        }
        let rows: Vec<AnyElement> = locations
            .into_iter()
            .enumerate()
            .map(|(ix, location)| {
                let selected = self.location.as_ref() == Some(&location);
                let name = location.display_name();
                let description = describe_location(&location);
                let forget = location.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pr_2()
                    .rounded_md()
                    .when(selected, |this| {
                        this.bg(theme.bg_active).text_color(theme.fg_0)
                    })
                    .child(
                        div()
                            .id((id, ix))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .child(div().truncate().child(name))
                            .child(
                                div()
                                    .truncate()
                                    .text_micro()
                                    .when(!selected, |this| this.text_color(theme.fg_1))
                                    .child(description),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select(location.clone(), window, cx)
                            })),
                    )
                    .when(forgettable, |this| {
                        this.child(
                            Button::new(("forget-recent", ix), strings::RECENT_FORGET)
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.forget(forget.clone(), cx)
                                })),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_secondary().text_color(theme.fg_1).child(title))
                .child(
                    div()
                        .id(id)
                        .max_h(px(220.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .into_any_element(),
        )
    }
}

/// Where a Vault file is, in full, saying which Storage provider holds it when it isn't local.
fn describe_location(location: &VaultLocation) -> String {
    match location {
        VaultLocation::Local(_) => location.description(),
        VaultLocation::Dropbox(path) => strings::DROPBOX_LOCATION.replace("{path}", path),
    }
}

/// Tells a wrong key, a damaged or unsupported Vault file and key file problems apart.
fn describe(error: &OpenVaultError) -> UnlockError {
    let (message, detail) = match error {
        OpenVaultError::AlreadyOpen(_) => (strings::UNLOCK_ERROR_ALREADY_OPEN, None),
        OpenVaultError::Vault(OpenError::WrongKey) => (strings::UNLOCK_ERROR_WRONG_KEY, None),
        OpenVaultError::Vault(OpenError::Corrupt(reason)) => {
            (strings::UNLOCK_ERROR_CORRUPT, Some(reason))
        }
        OpenVaultError::Vault(OpenError::Unsupported(reason)) => {
            (strings::UNLOCK_ERROR_UNSUPPORTED, Some(reason))
        }
        OpenVaultError::Storage(StorageError::NotFound(_)) => {
            (strings::UNLOCK_ERROR_NOT_FOUND, None)
        }
        OpenVaultError::Storage(StorageError::Io(_) | StorageError::Conflict) => {
            (strings::UNLOCK_ERROR_READ, None)
        }
        OpenVaultError::Storage(StorageError::Unavailable(_)) => {
            (strings::UNLOCK_ERROR_NO_PROVIDER, None)
        }
        OpenVaultError::Storage(StorageError::Offline(reason)) => {
            (strings::UNLOCK_ERROR_OFFLINE, Some(reason))
        }
        OpenVaultError::Storage(StorageError::Dropbox(error)) => {
            return UnlockError::new(dropbox_panel::describe(error));
        }
        OpenVaultError::KeyFileNotFound(_) => (strings::UNLOCK_ERROR_KEY_FILE_NOT_FOUND, None),
        OpenVaultError::KeyFileUnreadable(_) => (strings::UNLOCK_ERROR_KEY_FILE_READ, None),
        OpenVaultError::KeyFile(KeyFileError::Corrupt(reason)) => {
            (strings::UNLOCK_ERROR_KEY_FILE_CORRUPT, Some(reason))
        }
        OpenVaultError::KeyFile(KeyFileError::Unsupported(reason)) => {
            (strings::UNLOCK_ERROR_KEY_FILE_UNSUPPORTED, Some(reason))
        }
    };
    UnlockError {
        message: message.into(),
        detail: detail.map(|reason| reason.clone().into()),
    }
}

/// An indeterminate progress bar: key derivation can't report how far along it is.
fn progress_bar(theme: Theme) -> impl IntoElement {
    div()
        .relative()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(theme.border)
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(0.3))
                .rounded_full()
                .bg(theme.accent)
                .with_animation(
                    "unlock-progress",
                    Animation::new(Duration::from_millis(1200)).repeat(),
                    |bar, delta| bar.left(relative(delta * 1.3 - 0.3)),
                ),
        )
}

impl Render for UnlockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let can_unlock = self.location.is_some() && self.unlocking.is_none();
        let can_cancel = self.can_cancel(cx);
        let config_error = self.workspace.read(cx).config_error().is_some();
        let file_label: SharedString = match &self.location {
            Some(location) => describe_location(location).into(),
            None => strings::UNLOCK_NO_FILE.into(),
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
        if let Some(form) = self.new_vault.clone() {
            return div()
                .size_full()
                .py_6()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.bg_0)
                .text_color(theme.fg_0)
                .child(
                    div()
                        .id("new-vault-card")
                        .w(px(560.))
                        .max_h_full()
                        .overflow_y_scroll()
                        .p_6()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.bg_1)
                        .child(form),
                );
        }
        if let Some(dropbox) = self.dropbox.clone() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.bg_0)
                .text_color(theme.fg_0)
                .child(dropbox);
        }
        let has_dropbox = self.workspace.read(cx).dropbox().is_some();
        let locked = self.render_locked(theme, cx);
        let recents = self.render_recents(theme, cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.bg_0)
            .text_color(theme.fg_0)
            .child(
                div()
                    .w(px(460.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.bg_1)
                    .child(div().text_title().child(strings::UNLOCK_TITLE))
                    .children(locked)
                    .children(recents)
                    .child(
                        div()
                            .truncate()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .child(file_label),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .when(has_dropbox, |this| {
                                this.child(
                                    Button::new("from-dropbox", strings::UNLOCK_FROM_DROPBOX)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_from_dropbox(window, cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("new-vault", strings::UNLOCK_NEW_VAULT).on_click(
                                    cx.listener(|this, _, window, cx| this.new_vault(window, cx)),
                                ),
                            )
                            .child(
                                Button::new("choose-file", strings::UNLOCK_CHOOSE_FILE)
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .child(strings::UNLOCK_PASSWORD_LABEL),
                    )
                    .child(self.password.clone())
                    .child(
                        div()
                            .text_secondary()
                            .text_color(theme.fg_1)
                            .child(strings::UNLOCK_KEY_FILE_LABEL),
                    )
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
                                    .when(self.key_file.is_none(), |this| {
                                        this.text_color(theme.fg_1)
                                    })
                                    .child(key_file_label),
                            )
                            .when(self.key_file.is_some(), |this| {
                                this.child(
                                    Button::new("clear-key-file", strings::UNLOCK_CLEAR_KEY_FILE)
                                        .small()
                                        .ghost()
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.clear_key_file(cx)),
                                        ),
                                )
                            })
                            .child(
                                Button::new("choose-key-file", strings::UNLOCK_CHOOSE_KEY_FILE)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.choose_key_file(cx)),
                                    ),
                            ),
                    )
                    .when(self.unlocking.is_some(), |this| {
                        this.child(progress_bar(theme)).child(
                            div()
                                .text_micro()
                                .text_color(theme.fg_1)
                                .child(strings::UNLOCK_PROGRESS),
                        )
                    })
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_secondary()
                                        .text_color(theme.danger)
                                        .child(error.message),
                                )
                                .when_some(error.detail, |this, detail| {
                                    this.child(
                                        div().text_micro().text_color(theme.fg_1).child(detail),
                                    )
                                }),
                        )
                    })
                    .when(config_error, |this| {
                        this.child(
                            div()
                                .text_micro()
                                .text_color(theme.fg_1)
                                .child(strings::CONFIG_ERROR),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div().flex_1().flex().child(
                                    Button::new("open-settings", strings::OPEN_SETTINGS)
                                        .ghost()
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(OpenSettings), cx)
                                        }),
                                ),
                            )
                            .when(can_cancel, |this| {
                                this.child(
                                    Button::new("cancel-unlock", strings::CANCEL)
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                            })
                            .child(
                                Button::new(
                                    "unlock",
                                    if self.unlocking.is_some() {
                                        strings::UNLOCK_IN_PROGRESS
                                    } else {
                                        strings::UNLOCK_BUTTON
                                    },
                                )
                                .primary()
                                .kbd(Some(Kbd::new([strings::KEY_ENTER])))
                                .disabled(!can_unlock)
                                .on_click(cx.listener(|this, _, _, cx| this.unlock(cx))),
                            ),
                    ),
            )
    }
}
