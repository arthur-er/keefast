use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, HighlightStyle, KeyDownEvent,
    SharedString, StyledText, Window, div, prelude::*, px,
};
use keefast_core::{CharacterRecipe, GeneratorError, LOOKALIKES, PassphraseRecipe};
use keefast_workspace::{GeneratorMode, GeneratorSettings, Workspace};
use secrecy::{ExposeSecret, SecretString};

use crate::icons::IconName;
use crate::strings;
use crate::text_field::{TextField, TextFieldEvent};
use crate::theme::MONO_FONT;
use crate::theme::Theme;
use crate::ui::{self, Button, Kbd, Segmented, Slider, Typography};

pub enum PasswordGeneratorEvent {
    /// Use this password.
    Use(SecretString),
    Cancel,
}

/// The generator panel in the Entry editor: picks the mode and its settings, which the
/// Workspace remembers, and shows a password made with them. On the settings screen it only
/// shows what the settings make.
pub struct PasswordGenerator {
    workspace: Entity<Workspace>,
    separator: Entity<TextField>,
    generated: Result<SecretString, GeneratorError>,
    in_settings: bool,
    focus: FocusHandle,
}

impl EventEmitter<PasswordGeneratorEvent> for PasswordGenerator {}

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
            in_settings: false,
            focus: cx.focus_handle(),
        }
    }

    /// The panel for the settings screen, without a password to use.
    pub fn for_settings(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            in_settings: true,
            ..Self::new(workspace, cx)
        }
    }

    /// Changes the generator settings and makes a new password with them, unless nothing
    /// changed.
    fn change(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut GeneratorSettings)) {
        let mut settings = self.workspace.read(cx).generator_settings().clone();
        edit(&mut settings);
        let changed = self.workspace.update(cx, |workspace, _| {
            workspace.set_generator_settings(settings)
        });
        if changed {
            self.regenerate(cx);
        }
    }

    fn regenerate(&mut self, cx: &mut Context<Self>) {
        self.generated = self.workspace.read(cx).generate_password();
        cx.notify();
    }

    fn use_password(&mut self, cx: &mut Context<Self>) {
        if self.in_settings {
            return;
        }
        if let Ok(password) = &self.generated {
            cx.emit(PasswordGeneratorEvent::Use(password.clone()));
        }
    }
}

/// A setting kept within `min..=max`: a slider, and the number with `−` and `+` beside it.
fn amount(
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
    let generator = cx.entity().downgrade();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::field_label(label, theme))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(Slider::new(
                    SharedString::from(format!("{id}-slider")),
                    value,
                    (min, max),
                    move |value, _, cx| {
                        generator
                            .update(cx, |this, cx| {
                                this.change(cx, |settings| set(settings, value))
                            })
                            .ok();
                    },
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.border_strong)
                        .bg(theme.bg_1)
                        .child(
                            ui::IconButton::new(
                                SharedString::from(format!("{id}-less")),
                                IconName::Minus,
                            )
                            .small()
                            .on_click(step(-1)),
                        )
                        .child(
                            div()
                                .min_w(px(28.))
                                .flex()
                                .justify_center()
                                .font_family(MONO_FONT)
                                .child(value.to_string()),
                        )
                        .child(
                            ui::IconButton::new(
                                SharedString::from(format!("{id}-more")),
                                IconName::Plus,
                            )
                            .small()
                            .on_click(step(1)),
                        ),
                ),
        )
}

/// A kind of character to use or not, as a toggle.
fn class_toggle(
    id: &'static str,
    label: &'static str,
    on: bool,
    theme: Theme,
    cx: &Context<PasswordGenerator>,
    toggle: fn(&mut CharacterRecipe),
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .cursor_pointer()
        .font_family(MONO_FONT)
        .text_size(px(12.))
        .map(|this| {
            if on {
                this.bg(theme.accent_bg)
                    .border_color(theme.accent)
                    .text_color(theme.accent_fg)
            } else {
                this.bg(theme.bg_1)
                    .border_color(theme.border_strong)
                    .text_color(theme.fg_1)
                    .hover(|style| style.text_color(theme.fg_0))
            }
        })
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.change(cx, |settings| toggle(&mut settings.characters))
        }))
}

/// The generated password, its digits and symbols tinted so each character reads.
fn colored_password(password: &str, theme: Theme) -> StyledText {
    let mut highlights = Vec::new();
    for (at, ch) in password.char_indices() {
        let color = if ch.is_ascii_digit() {
            Some(theme.info)
        } else if !ch.is_alphanumeric() {
            Some(theme.accent_fg)
        } else {
            None
        };
        if let Some(color) = color {
            highlights.push((
                at..at + ch.len_utf8(),
                HighlightStyle {
                    color: Some(color.into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(SharedString::from(password.to_owned())).with_highlights(highlights)
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

impl PasswordGenerator {
    /// R makes another password, Enter uses it and Escape closes the popover.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "r" => self.regenerate(cx),
            "enter" => self.use_password(cx),
            "escape" => cx.emit(PasswordGeneratorEvent::Cancel),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Focusable for PasswordGenerator {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PasswordGenerator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let settings = self.workspace.read(cx).generator_settings().clone();
        let mode_tab = |mode: GeneratorMode| {
            cx.listener(move |this: &mut Self, _, _, cx| {
                this.change(cx, |settings| settings.mode = mode)
            })
        };
        let tabs = Segmented::new("generator-mode")
            .option(
                strings::GENERATOR_CHARACTERS,
                settings.mode == GeneratorMode::Characters,
                mode_tab(GeneratorMode::Characters),
            )
            .option(
                strings::GENERATOR_PASSPHRASE,
                settings.mode == GeneratorMode::Passphrase,
                mode_tab(GeneratorMode::Passphrase),
            );

        let options = match settings.mode {
            GeneratorMode::Characters => {
                let recipe = &settings.characters;
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(amount(
                        "generator-length",
                        strings::GENERATOR_LENGTH,
                        recipe.length,
                        (recipe.min_length(), CharacterRecipe::MAX_LENGTH),
                        theme,
                        cx,
                        |settings, length| settings.characters.length = length,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(class_toggle(
                                "generator-lowercase",
                                strings::GENERATOR_LOWERCASE,
                                recipe.lowercase,
                                theme,
                                cx,
                                |recipe| recipe.lowercase = !recipe.lowercase,
                            ))
                            .child(class_toggle(
                                "generator-uppercase",
                                strings::GENERATOR_UPPERCASE,
                                recipe.uppercase,
                                theme,
                                cx,
                                |recipe| recipe.uppercase = !recipe.uppercase,
                            ))
                            .child(class_toggle(
                                "generator-digits",
                                strings::GENERATOR_DIGITS,
                                recipe.digits,
                                theme,
                                cx,
                                |recipe| recipe.digits = !recipe.digits,
                            ))
                            .child(class_toggle(
                                "generator-symbols",
                                strings::GENERATOR_SYMBOLS,
                                recipe.symbols,
                                theme,
                                cx,
                                |recipe| recipe.symbols = !recipe.symbols,
                            )),
                    )
                    .child(
                        div()
                            .id("generator-lookalikes")
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .cursor_pointer()
                            .child(ui::switch_mark(recipe.exclude_lookalikes, theme))
                            .child(
                                strings::GENERATOR_EXCLUDE_LOOKALIKES.replace(
                                    "{lookalikes}",
                                    &LOOKALIKES
                                        .chars()
                                        .map(String::from)
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                ),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.change(cx, |settings| {
                                    let recipe = &mut settings.characters;
                                    recipe.exclude_lookalikes = !recipe.exclude_lookalikes;
                                })
                            })),
                    )
            }
            GeneratorMode::Passphrase => div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(amount(
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
                        .flex_col()
                        .gap(px(8.))
                        .child(ui::field_label(strings::GENERATOR_SEPARATOR, theme))
                        .child(div().w(px(120.)).child(self.separator.clone())),
                ),
        };

        let preview = div()
            .flex()
            .items_start()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg_inset)
            .child(match &self.generated {
                Ok(password) => div()
                    .flex_1()
                    .min_w_0()
                    .min_h(px(40.))
                    .font_family(MONO_FONT)
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .child(colored_password(password.expose_secret(), theme)),
                Err(error) => div()
                    .flex_1()
                    .min_h(px(40.))
                    .text_color(theme.danger)
                    .child(error_message(error)),
            })
            .child(
                ui::IconButton::new("generator-again-icon", IconName::RefreshCw)
                    .small()
                    .tooltip(strings::GENERATOR_AGAIN)
                    .on_click(cx.listener(|this, _, _, cx| this.regenerate(cx))),
            );

        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(14.))
            .child(div().flex().child(tabs))
            .child(preview)
            .child(options);

        if self.in_settings {
            return ui::card(theme).id("password-generator").child(body);
        }
        ui::menu(theme)
            .id("password-generator")
            .key_context("PasswordGenerator")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(400.))
            .p_0()
            .occlude()
            .child(body)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.bg_inset)
                    .rounded_b(px(8.))
                    .child(Kbd::new([strings::KEY_ESC]))
                    .child(
                        div()
                            .flex_1()
                            .text_secondary()
                            .text_color(theme.fg_2)
                            .child(strings::GENERATOR_CLOSE),
                    )
                    .child(
                        Button::new("generator-again", strings::GENERATOR_AGAIN)
                            .small()
                            .ghost()
                            .kbd(Some(Kbd::new(["R"])))
                            .on_click(cx.listener(|this, _, _, cx| this.regenerate(cx))),
                    )
                    .child(
                        Button::new("generator-use", strings::GENERATOR_USE)
                            .small()
                            .primary()
                            .kbd(Some(Kbd::new([strings::KEY_ENTER])))
                            .disabled(self.generated.is_err())
                            .on_click(cx.listener(|this, _, _, cx| this.use_password(cx))),
                    ),
            )
    }
}
