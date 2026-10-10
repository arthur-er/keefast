//! The design tokens every view draws with: keefast's dark and light colours, its two type
//! faces and its shadows. Views name a token, never a colour.

use std::borrow::Cow;

use gpui::{App, BoxShadow, Global, Rgba, Window, WindowAppearance, point, px};
use keefast_core::EntryColor;
use keefast_workspace::ThemeSetting;

/// The face of the interface.
pub const UI_FONT: &str = "Inter";
/// The face of secrets, codes, paths and keys.
pub const MONO_FONT: &str = "JetBrains Mono";

/// The type faces keefast draws with, so it looks the same on every desktop.
const FONTS: [&[u8]; 5] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
];

/// Makes keefast's type faces available to every window.
pub fn load_fonts(cx: &mut App) {
    let fonts = FONTS.into_iter().map(Cow::Borrowed).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("keefast: couldn't load its fonts: {error}");
    }
}

/// The theme the user picked, which every view draws with.
struct ChosenTheme(ThemeSetting);

impl Global for ChosenTheme {}

/// Draws every window in `setting`'s colours from now on.
pub fn choose(setting: ThemeSetting, cx: &mut App) {
    cx.set_global(ChosenTheme(setting));
    draw_kit_in(Theme::of(setting, cx.window_appearance()), cx);
}

/// Follows the OS light/dark setting as `window` sees it now, should the theme follow it.
pub fn follow_appearance(window: &Window, cx: &mut App) {
    draw_kit_in(Theme::for_window(window, cx), cx);
}

/// Brings gpui-kit, which draws the window's background, scrollbars and tooltips, onto
/// `theme`'s colours and keefast's type faces.
fn draw_kit_in(theme: Theme, cx: &mut App) {
    gpui_component::Theme::update(cx, |kit| {
        kit.font_family = UI_FONT.into();
        kit.mono_font_family = MONO_FONT.into();
        kit.mono_font_size = px(13.);
        kit.radius = px(6.);
        kit.radius_lg = px(8.);
        let colors = &mut kit.colors;
        colors.background = theme.bg_0.into();
        colors.foreground = theme.fg_0.into();
        colors.muted_foreground = theme.fg_1.into();
        colors.border = theme.border.into();
        colors.popover = theme.bg_popover.into();
        colors.popover_foreground = theme.fg_0.into();
        colors.accent = theme.bg_hover.into();
        colors.accent_foreground = theme.fg_0.into();
        colors.primary = theme.accent.into();
        colors.primary_foreground = theme.fg_on_accent.into();
        colors.ring = theme.accent.into();
        colors.selection = theme.accent_bg.into();
        colors.caret = theme.fg_0.into();
        colors.scrollbar = gpui::transparent_black();
        colors.scrollbar_thumb = theme.border_strong.into();
        colors.scrollbar_thumb_hover = theme.fg_2.into();
    });
    cx.refresh_windows();
}

/// The colours the views draw with: those the user picked, or else those following the OS
/// light/dark setting. Named as DESIGN.md names them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,

    /// Behind the Entry list and the Entry detail.
    pub bg_0: Rgba,
    /// Behind the sidebar, one step darker than the window.
    pub bg_sidebar: Rgba,
    /// Raised surfaces: cards, inputs, secondary buttons.
    pub bg_1: Rgba,
    /// A surface raised on a raised one.
    pub bg_2: Rgba,
    /// Behind menus and popovers.
    pub bg_popover: Rgba,
    /// Under the pointer: rows, ghost buttons.
    pub bg_hover: Rgba,
    /// The selected row.
    pub bg_active: Rgba,
    /// Sunk into a surface, as a code well.
    pub bg_inset: Rgba,

    /// Hairlines between panes and rows.
    pub border: Rgba,
    /// The edges of inputs and buttons.
    pub border_strong: Rgba,

    /// Titles and values.
    pub fg_0: Rgba,
    /// Labels and usernames.
    pub fg_1: Rgba,
    /// Hints, timestamps and placeholders.
    pub fg_2: Rgba,
    pub fg_on_accent: Rgba,

    /// The primary action, focus and the selection mark; nothing else.
    pub accent: Rgba,
    pub accent_hover: Rgba,
    /// A tint of the accent, behind accent text.
    pub accent_bg: Rgba,
    /// Accent-coloured text.
    pub accent_fg: Rgba,
    /// The outer glow of a focus ring.
    pub ring: Rgba,

    pub danger: Rgba,
    pub danger_bg: Rgba,
    pub warn: Rgba,
    pub warn_bg: Rgba,
    pub ok: Rgba,
    pub ok_bg: Rgba,
    pub info: Rgba,
    pub info_bg: Rgba,

    /// Over the window behind a dialog.
    pub scrim: Rgba,

    /// The Entry colours, in [`EntryColor`]'s order.
    labels: [Rgba; 6],
}

impl Theme {
    pub fn for_window(window: &Window, cx: &App) -> Self {
        let setting = cx
            .try_global::<ChosenTheme>()
            .map_or(ThemeSetting::System, |chosen| chosen.0);
        Self::of(setting, window.appearance())
    }

    /// The colours of `setting`, with System following `appearance`.
    pub fn of(setting: ThemeSetting, appearance: WindowAppearance) -> Self {
        match setting {
            ThemeSetting::System => match appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => DARK,
                WindowAppearance::Light | WindowAppearance::VibrantLight => LIGHT,
            },
            ThemeSetting::Dark => DARK,
            ThemeSetting::Light => LIGHT,
        }
    }

    /// The colour an Entry is labelled with.
    pub fn label(&self, color: EntryColor) -> Rgba {
        self.labels[match color {
            EntryColor::Red => 0,
            EntryColor::Orange => 1,
            EntryColor::Yellow => 2,
            EntryColor::Green => 3,
            EntryColor::Blue => 4,
            EntryColor::Violet => 5,
        }]
    }

    /// Under menus and popovers.
    pub fn shadow_popover(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                edge(white(0.08)),
                drop(8., 24., black(0.45)),
                drop(2., 6., black(0.3)),
            ]
        } else {
            vec![
                edge(black(0.07)),
                drop(8., 24., black(0.1)),
                drop(2., 6., black(0.06)),
            ]
        }
    }

    /// Under dialogs.
    pub fn shadow_dialog(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![
                edge(white(0.09)),
                drop(24., 64., black(0.6)),
                drop(4., 12., black(0.35)),
            ]
        } else {
            vec![
                edge(black(0.07)),
                drop(24., 64., black(0.16)),
                drop(4., 12., black(0.08)),
            ]
        }
    }

    /// Around the focused control: an accent line and a soft glow outside it.
    pub fn focus_ring(&self) -> Vec<BoxShadow> {
        vec![
            BoxShadow {
                color: self.ring.into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(3.),
                inset: false,
            },
            edge(self.accent),
        ]
    }
}

/// A 1px line around an element, drawn as a shadow so it takes no room.
fn edge(color: Rgba) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(1.),
        inset: false,
    }
}

fn drop(y: f32, blur: f32, color: Rgba) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: false,
    }
}

const fn white(alpha: f32) -> Rgba {
    Rgba {
        r: 1.,
        g: 1.,
        b: 1.,
        a: alpha,
    }
}

const fn black(alpha: f32) -> Rgba {
    Rgba {
        r: 0.,
        g: 0.,
        b: 0.,
        a: alpha,
    }
}

const DARK: Theme = Theme {
    dark: true,
    bg_0: rgb_const(0x0d0e10),
    bg_sidebar: rgb_const(0x0a0b0c),
    bg_1: rgb_const(0x141518),
    bg_2: rgb_const(0x1a1b1f),
    bg_popover: rgb_const(0x17181c),
    bg_hover: white(0.04),
    bg_active: white(0.07),
    bg_inset: black(0.25),
    border: white(0.07),
    border_strong: white(0.12),
    fg_0: rgb_const(0xededef),
    fg_1: rgb_const(0xa0a1a8),
    fg_2: rgb_const(0x6b6d76),
    fg_on_accent: rgb_const(0xffffff),
    accent: rgb_const(0xa970ff),
    accent_hover: rgb_const(0xb888ff),
    accent_bg: with_alpha(0xa970ff, 0.14),
    accent_fg: rgb_const(0xc6a3ff),
    ring: with_alpha(0xa970ff, 0.22),
    danger: rgb_const(0xf2555a),
    danger_bg: with_alpha(0xf2555a, 0.12),
    warn: rgb_const(0xf0b429),
    warn_bg: with_alpha(0xf0b429, 0.12),
    ok: rgb_const(0x3fbf7f),
    ok_bg: with_alpha(0x3fbf7f, 0.12),
    info: rgb_const(0x5b9cf5),
    info_bg: with_alpha(0x5b9cf5, 0.12),
    scrim: black(0.55),
    labels: LABELS,
};

const LIGHT: Theme = Theme {
    dark: false,
    bg_0: rgb_const(0xfcfcfc),
    bg_sidebar: rgb_const(0xf5f5f6),
    bg_1: rgb_const(0xffffff),
    bg_2: rgb_const(0xf3f3f4),
    bg_popover: rgb_const(0xffffff),
    bg_hover: black(0.035),
    bg_active: black(0.06),
    bg_inset: black(0.03),
    border: black(0.07),
    border_strong: black(0.12),
    fg_0: rgb_const(0x1b1b1f),
    fg_1: rgb_const(0x5f6068),
    fg_2: rgb_const(0x8e9099),
    fg_on_accent: rgb_const(0xffffff),
    accent: rgb_const(0x8a46e6),
    accent_hover: rgb_const(0x7a36d6),
    accent_bg: with_alpha(0x8a46e6, 0.1),
    accent_fg: rgb_const(0x7232cc),
    ring: with_alpha(0x8a46e6, 0.18),
    danger: rgb_const(0xd93a40),
    danger_bg: with_alpha(0xd93a40, 0.08),
    warn: rgb_const(0xb7791f),
    warn_bg: with_alpha(0xf0b429, 0.14),
    ok: rgb_const(0x23955e),
    ok_bg: with_alpha(0x23955e, 0.1),
    info: rgb_const(0x2f74d6),
    info_bg: with_alpha(0x2f74d6, 0.08),
    scrim: with_alpha(0x141419, 0.28),
    labels: LABELS,
};

/// The Entry colours read on both the dark and the light background.
const LABELS: [Rgba; 6] = [
    rgb_const(0xeb5757),
    rgb_const(0xf2994a),
    rgb_const(0xf2c94c),
    rgb_const(0x4cb782),
    rgb_const(0x4ea7fc),
    rgb_const(0xa970ff),
];

const fn rgb_const(hex: u32) -> Rgba {
    with_alpha(hex, 1.)
}

const fn with_alpha(hex: u32, alpha: f32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_theme_follows_the_os() {
        let background = |appearance| Theme::of(ThemeSetting::System, appearance).bg_0;
        assert_eq!(background(WindowAppearance::Dark), rgb_const(0x0d0e10));
        assert_eq!(background(WindowAppearance::Light), rgb_const(0xfcfcfc));
    }

    #[test]
    fn a_picked_theme_ignores_the_os() {
        let dark = Theme::of(ThemeSetting::Dark, WindowAppearance::Light);
        assert!(dark.dark);
        let light = Theme::of(ThemeSetting::Light, WindowAppearance::Dark);
        assert!(!light.dark);
    }
}
