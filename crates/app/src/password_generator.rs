use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Window, div,
    prelude::*,
};
use keefast_core::{CharacterRecipe, GeneratorError, PassphraseRecipe};
use keefast_workspace::{GeneratorMode, GeneratorSettings, Workspace};
use secrecy::{ExposeSecret, SecretString};

use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::Theme;
use crate::widgets::{button, tool_button};

pub enum PasswordGeneratorEvent {
    /// Use this password.
    Use(SecretString),
    Cancel,
}

/// The generator panel in the Entry editor: picks the mode and its settings, which the
/// Workspace remembers, and shows a password made with them.
pub struct PasswordGenerator {
    workspace: Entity<Workspace>,
    separator: Entity<TextField>,
    generated: Result<SecretString, GeneratorError>,
}

impl EventEmitter<PasswordGeneratorEvent> for PasswordGenerator {}

impl Focusable for PasswordGenerator {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.separator.focus_handle(cx)
    }
}

impl PasswordGenerator {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let separator_text = workspace
            .read(cx)
            .generator_settings()
            .passphrase
            .separator
            .clone();
        let separator = cx.new(|cx| {
            let mut field = TextField::new(strings::GENERATOR_SEPARATOR, cx);
            field.set_text(&separator_text, cx);
            field
        });
        cx.observe(&separator, |this, separator, cx| {
            let text = separator.read(cx).text().to_owned();
            this.change(cx, |settings| settings.passphrase.separator = text);
        })
        .detach();
        cx.subscribe(&separator, |this, _, event, cx| match event {
            TextFieldEvent::Submit => this.use_password(cx),
            TextFieldEvent::Cancel => cx.emit(PasswordGeneratorEvent::Cancel),
        })
        .detach();
        let generated = workspace.read(cx).generate_password();
        Self {
            workspace,
            separator,
            generated,
        }
    }

    /// Changes the generator settings and makes a new password with them, unless nothing
    /// changed.
    fn change(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut GeneratorSettings)) {
        let mut settings = self.workspace.read(cx).generator_settings().clone();
        let before = settings.clone();
        edit(&mut settings);
        if settings != before {
            self.workspace.update(cx, |workspace, _| {
                workspace.set_generator_settings(settings)
            });
            self.regenerate(cx);
        }
    }

    fn regenerate(&mut self, cx: &mut Context<Self>) {
        self.generated = self.workspace.read(cx).generate_password();
        cx.notify();
    }

    fn use_password(&mut self, cx: &mut Context<Self>) {
        if let Ok(password) = &self.generated {
            cx.emit(PasswordGeneratorEvent::Use(password.clone()));
        }
    }
}

/// A `−  N  +` stepper for a number kept within `min..=max`.
fn stepper(
    id: &'static str,
    label: &'static str,
    value: u32,
    (min, max): (u32, u32),
    theme: Theme,
    cx: &Context<PasswordGenerator>,
    set: fn(&mut GeneratorSettings, u32),
) -> impl IntoElement {
    let step = move |delta: i64| {
        let next = (i64::from(value) + delta).clamp(i64::from(min), i64::from(max)) as u32;
        cx.listener(move |this: &mut PasswordGenerator, _, _, cx| {
            this.change(cx, |settings| set(settings, next))
        })
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().text_sm().text_color(theme.muted).child(label))
        .child(tool_button(SharedString::from(format!("{id}-less")), "−", theme).on_click(step(-1)))
        .child(
            div()
                .min_w_8()
                .flex()
                .justify_center()
                .child(value.to_string()),
        )
        .child(tool_button(SharedString::from(format!("{id}-more")), "+", theme).on_click(step(1)))
}

fn checkbox(
    id: &'static str,
    label: &'static str,
    on: bool,
    cx: &Context<PasswordGenerator>,
    toggle: fn(&mut CharacterRecipe),
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .gap_1()
        .text_sm()
        .cursor_pointer()
        .child(if on {
            strings::CHECKBOX_ON
        } else {
            strings::CHECKBOX_OFF
        })
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.change(cx, |settings| toggle(&mut settings.characters))
        }))
}

fn error_message(error: &GeneratorError) -> String {
    match error {
        GeneratorError::NoCharacterClasses => strings::GENERATOR_ERROR_NO_CLASSES.to_owned(),
        GeneratorError::LengthOutOfRange { min, max } => strings::GENERATOR_ERROR_LENGTH
            .replace("{min}", &min.to_string())
            .replace("{max}", &max.to_string()),
        GeneratorError::WordCountOutOfRange { min, max } => strings::GENERATOR_ERROR_WORDS
            .replace("{min}", &min.to_string())
            .replace("{max}", &max.to_string()),
    }
}

impl Render for PasswordGenerator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let settings = self.workspace.read(cx).generator_settings().clone();
        let mode_tab = |id: &'static str, label: &'static str, mode: GeneratorMode| {
            let selected = settings.mode == mode;
            tool_button(id, label, theme)
                .when(selected, |this| {
                    this.bg(theme.accent).text_color(theme.accent_text)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change(cx, |settings| settings.mode = mode)
                }))
        };
        let tabs = div()
            .flex()
            .gap_1()
            .child(mode_tab(
                "generator-characters",
                strings::GENERATOR_CHARACTERS,
                GeneratorMode::Characters,
            ))
            .child(mode_tab(
                "generator-passphrase",
                strings::GENERATOR_PASSPHRASE,
                GeneratorMode::Passphrase,
            ));

        let options = match settings.mode {
            GeneratorMode::Characters => {
                let recipe = &settings.characters;
                let classes = [
                    recipe.lowercase,
                    recipe.uppercase,
                    recipe.digits,
                    recipe.symbols,
                ];
                let min = classes.iter().filter(|&&on| on).count().max(1) as u32;
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(stepper(
                        "generator-length",
                        strings::GENERATOR_LENGTH,
                        recipe.length,
                        (min, CharacterRecipe::MAX_LENGTH),
                        theme,
                        cx,
                        |settings, length| settings.characters.length = length,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_3()
                            .child(checkbox(
                                "generator-lowercase",
                                strings::GENERATOR_LOWERCASE,
                                recipe.lowercase,
                                cx,
                                |recipe| recipe.lowercase = !recipe.lowercase,
                            ))
                            .child(checkbox(
                                "generator-uppercase",
                                strings::GENERATOR_UPPERCASE,
                                recipe.uppercase,
                                cx,
                                |recipe| recipe.uppercase = !recipe.uppercase,
                            ))
                            .child(checkbox(
                                "generator-digits",
                                strings::GENERATOR_DIGITS,
                                recipe.digits,
                                cx,
                                |recipe| recipe.digits = !recipe.digits,
                            ))
                            .child(checkbox(
                                "generator-symbols",
                                strings::GENERATOR_SYMBOLS,
                                recipe.symbols,
                                cx,
                                |recipe| recipe.symbols = !recipe.symbols,
                            )),
                    )
                    .child(checkbox(
                        "generator-lookalikes",
                        strings::GENERATOR_EXCLUDE_LOOKALIKES,
                        recipe.exclude_lookalikes,
                        cx,
                        |recipe| recipe.exclude_lookalikes = !recipe.exclude_lookalikes,
                    ))
            }
            GeneratorMode::Passphrase => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(stepper(
                    "generator-words",
                    strings::GENERATOR_WORDS,
                    settings.passphrase.words,
                    (1, PassphraseRecipe::MAX_WORDS),
                    theme,
                    cx,
                    |settings, words| settings.passphrase.words = words,
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted)
                                .child(strings::GENERATOR_SEPARATOR),
                        )
                        .child(div().w_24().child(self.separator.clone())),
                ),
        };

        let preview = match &self.generated {
            Ok(password) => div()
                .font_family("monospace")
                .child(SharedString::from(password.expose_secret().to_owned())),
            Err(error) => div().text_color(theme.error).child(error_message(error)),
        };

        div()
            .id("password-generator")
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .child(tabs)
            .child(options)
            .child(
                div()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .bg(theme.background)
                    .overflow_hidden()
                    .child(preview),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(button("generator-cancel", strings::CANCEL, theme).on_click(
                        cx.listener(|_, _, _, cx| cx.emit(PasswordGeneratorEvent::Cancel)),
                    ))
                    .child(
                        button("generator-again", strings::GENERATOR_AGAIN, theme)
                            .on_click(cx.listener(|this, _, _, cx| this.regenerate(cx))),
                    )
                    .child(
                        button("generator-use", strings::GENERATOR_USE, theme)
                            .when(self.generated.is_ok(), |this| {
                                this.bg(theme.accent).text_color(theme.accent_text)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.use_password(cx))),
                    ),
            )
    }
}
