use gpui::{AnyElement, BoxShadow, Div, FontWeight, ParentElement, Rgba, Styled, div, point, px};

use crate::theme::Theme;

/// The tile an Entry is drawn as in the list and above its details: a monogram in a colour
/// picked from `seed`, the Entry's site or title, so each site keeps its colour.
pub fn monogram(seed: &str, size: f32) -> Div {
    let letter: String = seed
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    tile(size)
        .bg(MONOGRAM_COLORS[hash(seed) % MONOGRAM_COLORS.len()])
        .text_color(gpui::white())
        .text_size(px((size * 0.5).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .child(letter)
}

/// The tile with a picture in it, such as a favicon or a KeePass icon.
pub fn picture_tile(picture: AnyElement, size: f32, theme: Theme) -> Div {
    tile(size).bg(theme.bg_2).child(picture)
}

fn tile(size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(if size >= 40. { 10. } else { 6. }))
        .overflow_hidden()
        .shadow(vec![BoxShadow {
            color: gpui::white().opacity(0.1),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: true,
        }])
}

/// FNV-1a: the same seed always picks the same colour.
fn hash(seed: &str) -> usize {
    seed.to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
        }) as usize
}

/// Muted enough to sit quietly in a list, dark enough for white letters.
const MONOGRAM_COLORS: [Rgba; 8] = [
    rgb(0x4f5bd5),
    rgb(0x2f6fd0),
    rgb(0x0f8a7e),
    rgb(0x2b8a57),
    rgb(0xc2413f),
    rgb(0xc7651d),
    rgb(0x7c4bc4),
    rgb(0x55575f),
];

const fn rgb(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.,
        g: ((hex >> 8) & 0xff) as f32 / 255.,
        b: (hex & 0xff) as f32 / 255.,
        a: 1.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_keeps_its_colour_whatever_its_case() {
        assert_eq!(hash("github.com"), hash("GitHub.com"));
    }
}
