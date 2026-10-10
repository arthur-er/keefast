use gpui::{
    App, BoxShadow, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    point, prelude::FluentBuilder, px,
};

use crate::icons::{IconName, ui_icon};
use crate::theme::Theme;

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The track and knob of a switch, for controls that handle their own clicks and focus.
pub fn switch_mark(on: bool, theme: Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(30.))
        .h(px(18.))
        .p(px(2.))
        .rounded_full()
        .bg(if on {
            theme.accent
        } else {
            theme.border_strong
        })
        .flex()
        .when(on, |this| this.justify_end())
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(gpui::white())
                .shadow(vec![BoxShadow {
                    color: gpui::black().opacity(0.3),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
}

/// A box ticked or not, with its label beside it if given.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    on_click: Option<OnClick>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            label: None,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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

/// The box of a checkbox, ticked or not, for controls that handle their own clicks and focus.
pub fn check_mark(checked: bool, theme: Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded(px(4.))
        .border_1()
        .map(|this| {
            if checked {
                this.bg(theme.accent)
                    .border_color(theme.accent)
                    .child(ui_icon(IconName::Check, px(12.), theme.fg_on_accent))
            } else {
                this.bg(theme.bg_1).border_color(theme.border_strong)
            }
        })
}

/// The circle of a radio button, picked or not, for controls that handle their own clicks and
/// focus.
pub fn radio_mark(checked: bool, theme: Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.))
        .rounded_full()
        .bg(theme.bg_1)
        .border_1()
        .border_color(if checked {
            theme.accent
        } else {
            theme.border_strong
        })
        .when(checked, |this| {
            this.child(div().size(px(8.)).rounded_full().bg(theme.accent))
        })
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        let mark = check_mark(self.checked, theme);
        labelled(self.id, mark, self.label, theme)
            .when_some(self.on_click, |this, handler| this.on_click(handler))
    }
}

fn labelled(
    id: ElementId,
    mark: gpui::Div,
    label: Option<SharedString>,
    theme: Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .text_color(theme.fg_0)
        .child(mark)
        .children(label)
}

/// Two to four exclusive choices side by side, the picked one raised.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<(SharedString, bool, Option<OnClick>)>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            options: Vec::new(),
        }
    }

    pub fn option(
        mut self,
        label: impl Into<SharedString>,
        selected: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options
            .push((label.into(), selected, Some(Box::new(on_click))));
        self
    }
}

/// The sunk track a segmented control's choices sit in.
pub fn segment_track(theme: Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(7.))
        .bg(theme.bg_inset)
        .border_1()
        .border_color(theme.border)
}

/// Styles `choice` as one of a segmented control's: raised when `selected`.
pub fn segment<E: Styled + InteractiveElement>(choice: E, selected: bool, theme: Theme) -> E {
    let choice = choice
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.))
        .px(px(10.))
        .rounded(px(5.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer();
    if selected {
        choice
            .bg(theme.bg_2)
            .text_color(theme.fg_0)
            .shadow(vec![BoxShadow {
                color: theme.border_strong.into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
    } else {
        choice
            .text_color(theme.fg_1)
            .hover(|style| style.text_color(theme.fg_0))
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        segment_track(theme)
            .id(self.id)
            .children(self.options.into_iter().enumerate().map(
                move |(index, (label, selected, on_click))| {
                    segment(div().id(index), selected, theme)
                        .child(label)
                        .when_some(on_click, |this, handler| this.on_click(handler))
                },
            ))
    }
}
