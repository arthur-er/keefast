use gpui::{
    App, Div, FontWeight, IntoElement, ParentElement, RenderOnce, Rgba, SharedString, Styled,
    Window, div, px,
};

use crate::theme::Theme;

/// What a badge says about its subject.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Danger,
}

/// A small label: a state such as Expired or Locked, or a count.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: Tone,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: Tone::Neutral,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let (color, bg) = match self.tone {
            Tone::Neutral => (theme.fg_1, theme.bg_active),
            Tone::Danger => (theme.danger, theme.danger_bg),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(20.))
            .px(px(6.))
            .rounded(px(4.))
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .text_color(color)
            .bg(bg)
            .child(self.label)
    }
}

/// A round dot, as an Entry colour or a status.
pub fn dot(color: Rgba, size: f32) -> Div {
    div().size(px(size)).flex_none().rounded_full().bg(color)
}

/// A pill naming a tag, with a dot in front if `color` is given.
pub fn tag(label: impl Into<SharedString>, color: Option<Rgba>, theme: Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(22.))
        .px(px(8.))
        .rounded_full()
        .border_1()
        .border_color(theme.border_strong)
        .text_size(px(12.))
        .text_color(theme.fg_1)
        .whitespace_nowrap()
        .children(color.map(|color| dot(color, 6.)))
        .child(label.into())
}
