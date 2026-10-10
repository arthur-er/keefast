use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, ElementId, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, RenderOnce, Styled, Window, canvas, div,
    px, relative,
};

use crate::theme::Theme;

type OnChange = Rc<dyn Fn(u32, &mut Window, &mut App)>;

/// A track to drag a value along within `min..=max`, calling its handler with each value it
/// passes.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: u32,
    range: (u32, u32),
    on_change: OnChange,
}

impl Slider {
    pub fn new(
        id: impl Into<ElementId>,
        value: u32,
        range: (u32, u32),
        on_change: impl Fn(u32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            value,
            range,
            on_change: Rc::new(on_change),
        }
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::for_window(window, cx);
        // Whether a drag started on the track, kept from frame to frame while it lasts.
        let dragging = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let (min, max) = self.range;
        let span = max.saturating_sub(min).max(1) as f32;
        let fraction = (self.value.saturating_sub(min) as f32 / span).clamp(0., 1.);
        let set = self.on_change;
        div()
            .relative()
            .flex_1()
            .h(px(20.))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(8.))
                    .h(px(4.))
                    .rounded_full()
                    .bg(theme.border_strong)
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded_full()
                            .bg(theme.accent),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(relative(fraction))
                    .ml(px(-7.))
                    .size(px(14.))
                    .rounded_full()
                    .bg(gpui::white())
                    .border_1()
                    .border_color(theme.border_strong),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds: Bounds<Pixels>, (), window, _| {
                        let at = move |x: Pixels| {
                            let fraction = ((x - bounds.left()) / bounds.size.width).clamp(0., 1.);
                            min + (fraction * span).round() as u32
                        };
                        let (down_set, down_drag) = (set.clone(), dragging.clone());
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble
                                && event.button == MouseButton::Left
                                && bounds.contains(&event.position)
                            {
                                down_drag.update(cx, |dragging, _| *dragging = true);
                                down_set(at(event.position.x), window, cx);
                            }
                        });
                        let (move_set, move_drag) = (set.clone(), dragging.clone());
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble
                                && event.dragging()
                                && *move_drag.read(cx)
                            {
                                move_set(at(event.position.x), window, cx);
                            }
                        });
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                dragging.update(cx, |dragging, _| *dragging = false);
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
