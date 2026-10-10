use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, Styled,
    div, prelude::FluentBuilder, px,
};

use crate::theme::Theme;

/// A raised box grouping related rows or values.
pub fn card(theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg_1)
}

/// A row of a card: its name in a column, then its value.
pub fn kv_row(label: impl Into<SharedString>, first: bool, theme: Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .min_h(px(40.))
        .px(px(14.))
        .py(px(6.))
        .when(!first, |this| this.border_t_1().border_color(theme.border))
        .child(
            div()
                .w(px(92.))
                .flex_none()
                .text_size(px(13.))
                .text_color(theme.fg_2)
                .child(label.into()),
        )
}

/// A row of tabs over a pane, each with a count after its name if given.
pub fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    selected: bool,
    theme: Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(36.))
        .px(px(10.))
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .border_b_2()
        .map(|this| {
            if selected {
                this.border_color(theme.fg_0).text_color(theme.fg_0)
            } else {
                this.border_color(gpui::transparent_black())
                    .text_color(theme.fg_1)
                    .hover(|style| style.text_color(theme.fg_0))
            }
        })
        .child(label.into())
        .when_some(count, |this, count| {
            this.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.fg_2)
                    .child(SharedString::from(count.to_string())),
            )
        })
}
