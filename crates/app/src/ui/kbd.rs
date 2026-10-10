use gpui::{
    Action, App, FontWeight, IntoElement, KeybindingKeystroke, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, px,
};

use crate::strings;
use crate::theme::Theme;

/// The key caps of a shortcut, such as `Ctrl` `S`.
#[derive(IntoElement)]
pub struct Kbd {
    keys: Vec<SharedString>,
    /// Drawn on the accent, inside a primary button.
    on_accent: bool,
}

impl Kbd {
    /// Caps for keys already named, such as `["Esc"]`.
    pub fn new(keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
            on_accent: false,
        }
    }

    /// Caps for the keys bound to `action`, if any are: the binding made last, which takes
    /// precedence.
    pub fn for_action(action: &dyn Action, cx: &App) -> Option<Self> {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let binding = keymap.bindings_for_action(action).last()?;
        Some(Self::new(
            key_names(binding.keystrokes()).into_iter().flatten(),
        ))
    }

    pub fn on_accent(mut self) -> Self {
        self.on_accent = true;
        self
    }
}

/// The name of each key of each keystroke, modifiers first: `ctrl-shift-p` is
/// `[["Ctrl", "Shift", "P"]]`.
pub fn key_names(keystrokes: &[KeybindingKeystroke]) -> Vec<Vec<String>> {
    keystrokes
        .iter()
        .map(|keystroke| {
            let modifiers = keystroke.modifiers();
            let mut names: Vec<String> = [
                (modifiers.control, strings::KEY_CTRL),
                (modifiers.alt, strings::KEY_ALT),
                (modifiers.platform, strings::KEY_SUPER),
                (modifiers.shift, strings::KEY_SHIFT),
            ]
            .into_iter()
            .filter(|(held, _)| *held)
            .map(|(_, name)| name.to_owned())
            .collect();
            names.push(key_name(keystroke.key()));
            names
        })
        .collect()
}

fn key_name(key: &str) -> String {
    match key {
        "escape" => strings::KEY_ESC.to_owned(),
        "up" => "↑".to_owned(),
        "down" => "↓".to_owned(),
        "left" => "←".to_owned(),
        "right" => "→".to_owned(),
        _ => {
            let mut chars = key.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

impl RenderOnce for Kbd {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let (bg, border, color) = if self.on_accent {
            let white = |alpha| gpui::Rgba {
                r: 1.,
                g: 1.,
                b: 1.,
                a: alpha,
            };
            (white(0.16), white(0.2), theme.fg_on_accent)
        } else {
            (theme.bg_2, theme.border_strong, theme.fg_1)
        };
        div()
            .flex()
            .flex_none()
            .gap(px(3.))
            .children(self.keys.into_iter().map(|key| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_w(px(18.))
                    .h(px(18.))
                    .px(px(4.))
                    .rounded(px(4.))
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .text_color(color)
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(px(11.))
                    .child(key)
            }))
    }
}
