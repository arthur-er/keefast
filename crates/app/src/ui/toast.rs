use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, ElementId, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder, px, relative,
};

use crate::theme::Theme;

/// A short message at the bottom of the window, such as what was copied, with a bar running
/// down until what it speaks of runs out.
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    message: SharedString,
    countdown: Option<Duration>,
}

impl Toast {
    /// `id` tells one toast from the next, so each runs its own countdown.
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            countdown: None,
        }
    }

    pub fn countdown(mut self, countdown: Option<Duration>) -> Self {
        self.countdown = countdown;
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        div()
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(10.))
            .min_h(px(36.))
            .px(px(12.))
            .rounded(px(8.))
            .bg(theme.bg_popover)
            .shadow(theme.shadow_popover())
            .text_color(theme.fg_0)
            .text_size(px(13.))
            .child(self.message)
            .when_some(self.countdown, |this, countdown| {
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .h(px(2.))
                        .w_full()
                        .bg(theme.accent)
                        .with_animation(self.id, Animation::new(countdown), |bar, delta| {
                            bar.w(relative(1. - delta))
                        }),
                )
            })
    }
}
