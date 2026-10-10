use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Window, div,
    prelude::*, px,
};
use keefast_core::{Timestamp, Totp, TotpAlgorithm, TotpError};
use secrecy::ExposeSecret;

use crate::icons::IconName;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::MONO_FONT;
use crate::theme::Theme;
use crate::ui::{self, Button, Kbd, Segmented, Typography};

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
                let mut field = TextField::new(placeholder, cx).mono();
                field.set_text(value, cx);
                field
            });
            cx.subscribe(&field, Self::on_field_event).detach();
            field
        };
        let setup = Self {
            secret: field(strings::TOTP_SECRET_PLACEHOLDER, secret.expose_secret()),
            digits: field(strings::TOTP_DIGITS, &digits.to_string()),
            period: field(strings::TOTP_PERIOD, &period.to_string()),
            algorithm: current.map_or_else(TotpAlgorithm::default, Totp::algorithm),
            removable: current.is_some(),
            error: None,
        };
        // Keeps the code shown with the settings current.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        setup
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
}

impl TotpSetup {
    /// Puts the clipboard's text in the secret field, as a QR code's URI usually arrives.
    fn paste_secret(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let text = zeroize::Zeroizing::new(text);
        self.secret
            .update(cx, |field, cx| field.set_text(text.trim(), cx));
        self.error = None;
        cx.notify();
    }

    fn set_algorithm(&mut self, algorithm: TotpAlgorithm, cx: &mut Context<Self>) {
        self.algorithm = algorithm;
        cx.notify();
    }
}

impl Render for TotpSetup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let labelled = |text: &'static str, field: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(ui::field_label(text, theme))
                .child(field)
        };
        // The code the settings typed so far make, to check them against the site's.
        let now = Timestamp::from_system_time(SystemTime::now());
        let preview = self.totp(cx).ok().map(|totp| totp.code(now));
        let algorithm = |algorithm: TotpAlgorithm| {
            cx.listener(move |this: &mut Self, _, _, cx| this.set_algorithm(algorithm, cx))
        };

        ui::card(theme)
            .id("totp-setup")
            .border_color(theme.border_strong)
            .shadow(theme.shadow_popover())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .pt(px(12.))
                    .child(div().text_heading().child(strings::TOTP_SETUP_TITLE))
                    .child(
                        ui::IconButton::new("totp-close", IconName::X)
                            .small()
                            .tooltip(strings::CANCEL)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(TotpSetupEvent::Cancel))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(ui::field_label(strings::TOTP_SECRET, theme))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().child(self.secret.clone()))
                                    .child(
                                        Button::new("totp-paste", strings::TOTP_PASTE)
                                            .icon(IconName::Clipboard)
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.paste_secret(cx)),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_secondary()
                                    .text_color(theme.fg_2)
                                    .child(strings::TOTP_URI_NOTE),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(div().w(px(90.)).child(labelled(
                                strings::TOTP_DIGITS,
                                self.digits.clone().into_any_element(),
                            )))
                            .child(div().w(px(130.)).child(labelled(
                                strings::TOTP_PERIOD,
                                self.period.clone().into_any_element(),
                            )))
                            .child(
                                div().flex_1().child(labelled(
                                    strings::TOTP_ALGORITHM,
                                    div()
                                        .h(px(32.))
                                        .flex()
                                        .items_center()
                                        .child(
                                            Segmented::new("totp-algorithm")
                                                .option(
                                                    strings::TOTP_SHA1,
                                                    self.algorithm == TotpAlgorithm::Sha1,
                                                    algorithm(TotpAlgorithm::Sha1),
                                                )
                                                .option(
                                                    strings::TOTP_SHA256,
                                                    self.algorithm == TotpAlgorithm::Sha256,
                                                    algorithm(TotpAlgorithm::Sha256),
                                                )
                                                .option(
                                                    strings::TOTP_SHA512,
                                                    self.algorithm == TotpAlgorithm::Sha512,
                                                    algorithm(TotpAlgorithm::Sha512),
                                                ),
                                        )
                                        .into_any_element(),
                                )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .h(px(48.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.bg_inset)
                            .child(
                                div()
                                    .text_secondary()
                                    .text_color(theme.fg_2)
                                    .child(strings::TOTP_CODE_NOW),
                            )
                            .map(|this| match preview {
                                Some(code) => this
                                    .child(
                                        div()
                                            .flex_1()
                                            .font_family(MONO_FONT)
                                            .text_size(px(18.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(crate::vault_browser::grouped(code.code())),
                                    )
                                    .child(ui::countdown_ring(
                                        code.remaining() as f32 / code.period() as f32,
                                        16.,
                                        if code.remaining() <= 5 {
                                            theme.warn
                                        } else {
                                            theme.accent
                                        },
                                        theme.border_strong,
                                    ))
                                    .child(
                                        div().text_secondary().text_color(theme.fg_2).child(
                                            strings::TOTP_SECONDS_LEFT.replace(
                                                "{seconds}",
                                                &code.remaining().to_string(),
                                            ),
                                        ),
                                    ),
                                None => this.child(
                                    div()
                                        .flex_1()
                                        .text_color(theme.fg_2)
                                        .child(strings::TOTP_NO_CODE),
                                ),
                            }),
                    )
                    .when_some(self.error, |this, error| {
                        this.child(ui::error_text(describe(error), theme))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.bg_inset)
                    .rounded_b(px(8.))
                    .child(div().flex_1().flex().when(self.removable, |this| {
                        this.child(
                            Button::new("totp-remove", strings::TOTP_REMOVE)
                                .small()
                                .danger_ghost()
                                .icon(IconName::Trash)
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(TotpSetupEvent::Remove)),
                                ),
                        )
                    }))
                    .child(
                        Button::new("totp-cancel", strings::CANCEL)
                            .small()
                            .ghost()
                            .kbd(Some(Kbd::new([strings::KEY_ESC])))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(TotpSetupEvent::Cancel))),
                    )
                    .child(
                        Button::new("totp-save", strings::EDITOR_DONE)
                            .small()
                            .primary()
                            .kbd(Some(Kbd::new([strings::KEY_ENTER])))
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
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
