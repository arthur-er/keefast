use gpui::{
    AnyElement, App, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window,
    div, prelude::FluentBuilder, px,
};

use crate::theme::Theme;

use super::Typography;

/// A centred dialog for a decision that blocks: a title and a line saying what is asked, what
/// it's about, and the answers, primary last, at the bottom right.
#[derive(IntoElement)]
pub struct Dialog {
    title: SharedString,
    description: Option<SharedString>,
    body: Vec<AnyElement>,
    footer: Vec<AnyElement>,
    width: Pixels,
}

impl Dialog {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            body: Vec::new(),
            footer: Vec::new(),
            width: px(460.),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    /// An answer, in order: put the primary one last.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.footer.push(action.into_any_element());
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        div()
            .w(self.width)
            .max_w_full()
            .flex()
            .flex_col()
            .rounded(px(12.))
            .bg(theme.bg_popover)
            .shadow(theme.shadow_dialog())
            .text_color(theme.fg_0)
            .overflow_hidden()
            .child(
                div()
                    .pt(px(20.))
                    .px(px(20.))
                    .child(div().text_dialog_title().child(self.title))
                    .when_some(self.description, |this, description| {
                        this.child(
                            div()
                                .mt(px(6.))
                                .text_body()
                                .text_color(theme.fg_1)
                                .child(description),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .px(px(20.))
                    .py(px(16.))
                    .children(self.body),
            )
            .when(!self.footer.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .px(px(20.))
                        .py(px(12.))
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.bg_inset)
                        .children(self.footer),
                )
            })
    }
}
