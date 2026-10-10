use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, Styled,
    div, prelude::FluentBuilder, px,
};

use crate::icons::{IconName, ui_icon};
use crate::theme::Theme;

/// A 28px row of the sidebar, picked or not. Callers add its icon, label and count.
pub fn nav_row(id: impl Into<ElementId>, selected: bool, theme: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .px(px(8.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .map(|this| {
            if selected {
                this.bg(theme.bg_active).text_color(theme.fg_0)
            } else {
                this.text_color(theme.fg_1)
                    .hover(|style| style.bg(theme.bg_hover).text_color(theme.fg_0))
            }
        })
}

/// A heading over a section of the sidebar or of a list.
pub fn section_heading(label: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .h(px(26.))
        .px(px(8.))
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.fg_2)
        .child(label.into())
}

/// What a pane shows when it has nothing to show: one icon and one sentence.
pub fn empty_state(icon: IconName, message: impl Into<SharedString>, theme: Theme) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .text_size(px(13.))
        .text_color(theme.fg_2)
        .child(ui_icon(icon, px(20.), theme.fg_2))
        .child(message.into())
}
