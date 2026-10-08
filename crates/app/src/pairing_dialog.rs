use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, SharedString, Window, div, prelude::*, px,
};

use keefast_workspace::PairingId;

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::widgets::button;

pub enum PairingDialogEvent {
    Approve(String),
    Deny,
}

/// Asks the user whether to pair a browser extension with a Vault, and what to call it.
pub struct PairingDialog {
    pairing: PairingId,
    vault_name: SharedString,
    name: gpui::Entity<TextField>,
    error: Option<SharedString>,
}

impl EventEmitter<PairingDialogEvent> for PairingDialog {}

impl Focusable for PairingDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

impl PairingDialog {
    pub fn new(pairing: PairingId, vault_name: String, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| TextField::new(strings::PAIRING_NAME_PLACEHOLDER, cx));
        cx.subscribe(&name, |this, _, event, cx| match event {
            TextFieldEvent::Submit => this.approve(cx),
            TextFieldEvent::Cancel => cx.emit(PairingDialogEvent::Deny),
        })
        .detach();
        Self {
            pairing,
            vault_name: vault_name.into(),
            name,
            error: None,
        }
    }

    /// The Pairing request this dialog answers.
    pub fn pairing(&self) -> PairingId {
        self.pairing
    }

    pub fn set_error(&mut self, error: &'static str, cx: &mut Context<Self>) {
        self.error = Some(error.into());
        cx.notify();
    }

    fn approve(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().to_owned();
        cx.emit(PairingDialogEvent::Approve(name));
    }
}

impl Render for PairingDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        div()
            .id("pairing-dialog")
            .w(px(440.))
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .text_color(theme.text)
            .shadow_lg()
            .child(div().text_xl().child(strings::PAIRING_TITLE))
            .child(div().child(strings::PAIRING_PROMPT.replace("{vault}", &self.vault_name)))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(strings::PAIRING_NAME_LABEL),
            )
            .child(self.name.clone())
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("deny-pairing", strings::PAIRING_DENY, theme)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PairingDialogEvent::Deny))),
                    )
                    .child(
                        button("approve-pairing", strings::PAIRING_APPROVE, theme)
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .on_click(cx.listener(|this, _, _, cx| this.approve(cx))),
                    ),
            )
    }
}
