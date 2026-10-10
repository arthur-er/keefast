use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Window, div, prelude::*};
use keefast_core::{Totp, TotpAlgorithm, TotpError};
use secrecy::ExposeSecret;

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::ui::{Button, Typography};

pub enum TotpSetupEvent {
    Save(Totp),
    Remove,
    Cancel,
}

/// The form for setting up an Entry's TOTP, shown in the detail pane: a secret with its
/// settings by hand, or an `otpauth://` URI with its own.
pub struct TotpSetup {
    secret: Entity<TextField>,
    digits: Entity<TextField>,
    period: Entity<TextField>,
    algorithm: TotpAlgorithm,
    /// Whether the Entry has TOTP to remove.
    removable: bool,
    error: Option<TotpError>,
}

impl EventEmitter<TotpSetupEvent> for TotpSetup {}

impl Focusable for TotpSetup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.secret.focus_handle(cx)
    }
}

impl TotpSetup {
    /// A form filled with the Entry's TOTP, if it has one that can be read. Steam TOTPs, which
    /// have no settings by hand, are shown as their URI.
    pub fn new(
        current: Option<&Totp>,
        title: &str,
        username: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let secret = match current {
            Some(totp) if totp.is_steam() => totp.to_uri(title, username),
            Some(totp) => totp.secret(),
            None => String::new().into(),
        };
        let digits = current.map_or(Totp::DEFAULT_DIGITS, Totp::digits);
        let period = current.map_or(Totp::DEFAULT_PERIOD, Totp::period);
        let mut field = |placeholder: &'static str, value: &str| {
            let field = cx.new(|cx| {
                let mut field = TextField::new(placeholder, cx);
                field.set_text(value, cx);
                field
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        Self {
            secret: field(strings::TOTP_SECRET_PLACEHOLDER, secret.expose_secret()),
            digits: field(strings::TOTP_DIGITS, &digits.to_string()),
            period: field(strings::TOTP_PERIOD, &period.to_string()),
            algorithm: current.map_or_else(TotpAlgorithm::default, Totp::algorithm),
            removable: current.is_some(),
            error: None,
        }
    }

    fn on_field_event(
        &mut self,
        _: Entity<TextField>,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submit => self.save(cx),
            TextFieldEvent::Cancel => cx.emit(TotpSetupEvent::Cancel),
        }
    }

    /// The TOTP the form describes. An `otpauth://` URI brings its own settings.
    fn totp(&self, cx: &App) -> Result<Totp, TotpError> {
        let text = |field: &Entity<TextField>| field.read(cx).text();
        Totp::from_setup(
            text(&self.secret),
            self.algorithm,
            text(&self.digits),
            text(&self.period),
        )
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        match self.totp(cx) {
            Ok(totp) => cx.emit(TotpSetupEvent::Save(totp)),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn next_algorithm(&mut self, cx: &mut Context<Self>) {
        self.algorithm = match self.algorithm {
            TotpAlgorithm::Sha1 => TotpAlgorithm::Sha256,
            TotpAlgorithm::Sha256 => TotpAlgorithm::Sha512,
            TotpAlgorithm::Sha512 => TotpAlgorithm::Sha1,
        };
        cx.notify();
    }
}

impl Render for TotpSetup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let label = |text: &'static str| div().text_secondary().text_color(theme.fg_1).child(text);
        let labelled = |text: &'static str, field: &Entity<TextField>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(label(text))
                .child(field.clone())
        };

        div()
            .id("totp-setup")
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_dialog_title().child(strings::TOTP_SETUP_TITLE))
            .child(labelled(strings::TOTP_SECRET, &self.secret))
            .child(
                div()
                    .text_micro()
                    .text_color(theme.fg_1)
                    .child(strings::TOTP_URI_NOTE),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .child(labelled(strings::TOTP_DIGITS, &self.digits)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(labelled(strings::TOTP_PERIOD, &self.period)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(label(strings::TOTP_ALGORITHM))
                            .child(
                                Button::new("totp-algorithm", algorithm_name(self.algorithm))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.next_algorithm(cx)),
                                    ),
                            ),
                    ),
            )
            .when_some(self.error, |this, error| {
                this.child(
                    div()
                        .text_secondary()
                        .text_color(theme.danger)
                        .child(describe(error)),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .when(self.removable, |this| {
                        this.child(
                            Button::new("totp-remove", strings::TOTP_REMOVE).on_click(
                                cx.listener(|_, _, _, cx| cx.emit(TotpSetupEvent::Remove)),
                            ),
                        )
                    })
                    .child(
                        Button::new("totp-cancel", strings::CANCEL)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(TotpSetupEvent::Cancel))),
                    )
                    .child(
                        Button::new("totp-save", strings::EDITOR_DONE)
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}

fn algorithm_name(algorithm: TotpAlgorithm) -> &'static str {
    match algorithm {
        TotpAlgorithm::Sha1 => strings::TOTP_SHA1,
        TotpAlgorithm::Sha256 => strings::TOTP_SHA256,
        TotpAlgorithm::Sha512 => strings::TOTP_SHA512,
    }
}

pub fn describe(error: TotpError) -> &'static str {
    match error {
        TotpError::MissingSecret => strings::TOTP_ERROR_MISSING_SECRET,
        TotpError::InvalidSecret => strings::TOTP_ERROR_INVALID_SECRET,
        TotpError::InvalidDigits => strings::TOTP_ERROR_DIGITS,
        TotpError::InvalidPeriod => strings::TOTP_ERROR_PERIOD,
        TotpError::NotTotp => strings::TOTP_ERROR_NOT_TOTP,
    }
}
