use gpui::{
    Bounds, ParentElement, PathBuilder, Pixels, Point, Rgba, Styled, canvas, div, point, px,
};

/// A small ring that empties as `left` (0 to 1) runs down, as a TOTP code's time does.
pub fn countdown_ring(left: f32, size: f32, color: Rgba, track: Rgba) -> impl gpui::IntoElement {
    let left = left.clamp(0., 1.);
    div().size(px(size)).flex_none().child(
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, (), window, _| {
                let width = px(2.5);
                let radius = (bounds.size.width.min(bounds.size.height) - width) / 2.;
                let center = bounds.center();
                let arc = |from: f32, to: f32| {
                    let mut path = PathBuilder::stroke(width);
                    let steps = 48;
                    for step in 0..=steps {
                        let turn = from + (to - from) * step as f32 / steps as f32;
                        let angle = (turn - 0.25) * std::f32::consts::TAU;
                        let at: Point<Pixels> = point(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        );
                        if step == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    path.build().ok()
                };
                if let Some(path) = arc(0., 1.) {
                    window.paint_path(path, track);
                }
                if left > 0.
                    && let Some(path) = arc(1. - left, 1.)
                {
                    window.paint_path(path, color);
                }
            },
        )
        .size_full(),
    )
}
