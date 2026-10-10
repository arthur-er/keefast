//! keefast's building blocks, drawn as DESIGN.md describes them: buttons, key caps, form
//! controls, badges, menus, dialogs and toasts, each reading the theme's tokens. Views build
//! from these instead of styling `div`s.

mod badge;
mod button;
mod controls;
mod dialog;
mod kbd;
mod menu;
mod nav;
mod toast;
mod typography;

pub use badge::{Badge, Tone, dot, tag};
pub use button::{Button, IconButton};
pub use controls::{
    Checkbox, Segmented, check_mark, radio_mark, segment, segment_track, switch_mark,
};
pub use dialog::Dialog;
pub use kbd::{Kbd, key_names};
pub use menu::{MenuItem, menu};
pub use nav::{empty_state, nav_row, section_heading};
pub use toast::Toast;
pub use typography::Typography;

use gpui::{
    Animation, AnimationExt as _, Div, ElementId, IntoElement, ParentElement as _, Styled,
    ease_out_quint,
};
use std::time::Duration;

/// How long popovers and dialogs take to appear.
pub const APPEAR: Duration = Duration::from_millis(140);

/// Fades `element` in as it appears, as menus, popovers and dialogs do.
pub fn appear<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(APPEAR).with_easing(ease_out_quint()),
        |element, delta| element.opacity(delta),
    )
}

/// The label over a form field: 12px medium.
pub fn field_label(label: impl Into<gpui::SharedString>, theme: crate::theme::Theme) -> Div {
    gpui::div()
        .text_size(gpui::px(12.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.fg_1)
        .child(label.into())
}

/// An error under a field or in a dialog.
pub fn error_text(error: impl Into<gpui::SharedString>, theme: crate::theme::Theme) -> Div {
    gpui::div()
        .text_size(gpui::px(12.))
        .text_color(theme.danger)
        .child(error.into())
}
