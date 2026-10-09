use gpui::{ElementId, SharedString, Stateful, div, prelude::*};

use crate::theme::Theme;

/// A bordered, clickable label.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .child(label.into())
}

/// A compact button for toolbars.
pub fn tool_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_md()
        .text_sm()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .child(label.into())
}
