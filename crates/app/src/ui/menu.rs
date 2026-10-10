use gpui::{
    App, ClickEvent, Div, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};

use crate::icons::{IconName, ui_icon};
use crate::theme::Theme;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The raised box a menu's items sit in.
pub fn menu(theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(200.))
        .p(px(4.))
        .rounded(px(8.))
        .bg(theme.bg_popover)
        .shadow(theme.shadow_popover())
        .text_color(theme.fg_0)
        .text_size(px(13.))
}

/// A choice in a menu: a label, with a tick in front if it has one.
#[derive(IntoElement)]
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    checked: Option<bool>,
    on_click: Option<OnClick>,
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            checked: None,
            on_click: None,
        }
    }

    /// Keeps room for a tick in front, drawn when `checked`.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for MenuItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(30.))
            .px(px(8.))
            .rounded(px(5.))
            .cursor_pointer()
            .text_color(theme.fg_0)
            .hover(|style| style.bg(theme.bg_active))
            .when_some(self.checked, |this, checked| {
                this.child(div().size(px(15.)).flex_none().when(checked, |this| {
                    this.child(ui_icon(IconName::Check, px(15.), theme.fg_1))
                }))
            })
            .child(div().flex_1().min_w_0().truncate().child(self.label))
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}
