use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, PathPromptOptions, SharedString, Task,
    Window, div, prelude::*, px,
};
use keefast_core::{CompositeKey, OpenError};
use keefast_workspace::{OpenVaultError, StorageError, VaultId, VaultLocation, Workspace};

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::widgets::button;

/// Emitted once a Vault has been decrypted and added to the Workspace.
pub struct Unlocked(pub VaultId);

/// The unlock dialog: pick a Vault file, type the master password, unlock off the UI thread.
pub struct UnlockView {
    workspace: gpui::Entity<Workspace>,
    path: Option<PathBuf>,
    password: Entity<TextField>,
    error: Option<SharedString>,
    unlocking: Option<Task<()>>,
}

impl EventEmitter<Unlocked> for UnlockView {}

impl Focusable for UnlockView {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.password.focus_handle(cx)
    }
}

impl UnlockView {
    pub fn new(
        workspace: gpui::Entity<Workspace>,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let password =
            cx.new(|cx| TextField::new(strings::UNLOCK_PASSWORD_PLACEHOLDER, cx).masked());
        cx.subscribe(&password, |this, _, event, cx| match event {
            TextFieldEvent::Submit => this.unlock(cx),
            TextFieldEvent::Cancel => {}
        })
        .detach();
        Self {
            workspace,
            path,
            password,
            error: None,
            unlocking: None,
        }
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(strings::UNLOCK_CHOOSE_FILE_PROMPT.into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = paths.await;
            this.update(cx, |this, cx| {
                match picked {
                    Ok(Ok(Some(mut paths))) if !paths.is_empty() => {
                        this.path = Some(paths.swap_remove(0));
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

    fn unlock(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.unlocking.is_some() {
            return;
        }
        let key = CompositeKey::password(self.password.read(cx).text());
        let pending = self
            .workspace
            .read(cx)
            .begin_open(VaultLocation::Local(path), key);
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
                        this.password.update(cx, |field, cx| field.set_text("", cx));
                        let vault = this
                            .workspace
                            .update(cx, |workspace, _| workspace.add_unlocked(unlocked));
                        cx.emit(Unlocked(vault));
                    }
                    Err(error) => this.error = Some(describe(&error).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

fn describe(error: &OpenVaultError) -> &'static str {
    match error {
        OpenVaultError::Vault(OpenError::WrongKey) => strings::UNLOCK_ERROR_WRONG_KEY,
        OpenVaultError::Vault(OpenError::Corrupt(_)) => strings::UNLOCK_ERROR_CORRUPT,
        OpenVaultError::Vault(OpenError::Unsupported(_)) => strings::UNLOCK_ERROR_UNSUPPORTED,
        OpenVaultError::Storage(StorageError::NotFound(_)) => strings::UNLOCK_ERROR_NOT_FOUND,
        OpenVaultError::Storage(StorageError::Io(_)) => strings::UNLOCK_ERROR_READ,
    }
}

impl Render for UnlockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let can_unlock = self.path.is_some() && self.unlocking.is_none();
        let file_label: SharedString = match &self.path {
            Some(path) => path.display().to_string().into(),
            None => strings::UNLOCK_NO_FILE.into(),
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .text_color(theme.text)
            .child(
                div()
                    .w(px(420.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .child(div().text_xl().child(strings::UNLOCK_TITLE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .text_color(theme.muted)
                                    .child(file_label),
                            )
                            .child(
                                button("choose-file", strings::UNLOCK_CHOOSE_FILE, theme)
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted)
                            .child(strings::UNLOCK_PASSWORD_LABEL),
                    )
                    .child(self.password.clone())
                    .when_some(self.error.clone(), |this, error| {
                        this.child(div().text_sm().text_color(theme.error).child(error))
                    })
                    .child(
                        div().flex().justify_end().child(
                            button(
                                "unlock",
                                if self.unlocking.is_some() {
                                    strings::UNLOCK_IN_PROGRESS
                                } else {
                                    strings::UNLOCK_BUTTON
                                },
                                theme,
                            )
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .when(!can_unlock, |this| this.opacity(0.5))
                            .on_click(cx.listener(|this, _, _, cx| this.unlock(cx))),
                        ),
                    ),
            )
    }
}
