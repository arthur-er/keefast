use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, SharedString, Window, div, prelude::*, px,
};

use keefast_workspace::PairingId;

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{self, Button, Dialog};

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
        let theme = Theme::for_window(window, cx);
        div().id("pairing-dialog").child(ui::appear(
            "pairing-dialog-appear",
            div().child(
                Dialog::new(strings::PAIRING_TITLE)
                    .description(strings::PAIRING_PROMPT.replace("{vault}", &self.vault_name))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(ui::field_label(strings::PAIRING_NAME_LABEL, theme))
                            .child(self.name.clone()),
                    )
                    .when_some(self.error.clone(), |this, error| {
                        this.child(ui::error_text(error, theme))
                    })
                    .action(
                        Button::new("deny-pairing", strings::PAIRING_DENY)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PairingDialogEvent::Deny))),
                    )
                    .action(
                        Button::new("approve-pairing", strings::PAIRING_APPROVE)
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.approve(cx))),
                    ),
            ),
        ))
    }
}
