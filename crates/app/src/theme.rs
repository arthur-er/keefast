use gpui::{App, Global, Rgba, Window, WindowAppearance};
use keefast_workspace::ThemeSetting;

/// The theme the user picked, which every view draws with.
struct ChosenTheme(ThemeSetting);

impl Global for ChosenTheme {}

/// Draws every window in `setting`'s colours from now on.
pub fn choose(setting: ThemeSetting, cx: &mut App) {
    cx.set_global(ChosenTheme(setting));
    cx.refresh_windows();
}

/// The colours the views draw with: those the user picked, or else those following the OS
/// light/dark setting.
#[derive(Clone, Copy)]
pub struct Theme {
    pub background: Rgba,
    pub sidebar: Rgba,
    pub panel: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    /// Behind the picked Entry, and other picked rows on the background.
    pub selection: Rgba,
    pub selection_text: Rgba,
    /// Behind the picked row of the sidebar.
    pub sidebar_selection: Rgba,
    pub accent: Rgba,
    pub accent_text: Rgba,
    pub error: Rgba,
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
        let palette = match setting {
            ThemeSetting::System => match appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => &DARK,
                WindowAppearance::Light | WindowAppearance::VibrantLight => &LIGHT,
            },
            ThemeSetting::Dark => &DARK,
            ThemeSetting::Light => &LIGHT,
            ThemeSetting::SolarizedDark => &SOLARIZED_DARK,
            ThemeSetting::SolarizedLight => &SOLARIZED_LIGHT,
            ThemeSetting::DarkBlue => &DARK_BLUE,
            ThemeSetting::LightBlue => &LIGHT_BLUE,
            ThemeSetting::DarkBrown => &DARK_BROWN,
            ThemeSetting::LightBrown => &LIGHT_BROWN,
            ThemeSetting::Terminal => &TERMINAL,
            ThemeSetting::TerminalLight => &TERMINAL_LIGHT,
            ThemeSetting::DarkContrast => &DARK_CONTRAST,
            ThemeSetting::HighContrast => &HIGH_CONTRAST,
        };
        palette.theme()
    }
}

/// A theme as KeeWeb defines it: five colours the rest are mixed from, and the few colours
/// some themes pick by hand instead.
struct Palette {
    background: u32,
    /// Between the background and the text: muted text and borders are mixed from it.
    medium: u32,
    text: u32,
    action: u32,
    error: u32,
    /// How much of `medium` is in muted text.
    mute: f32,
    /// How much of `medium` is in borders.
    light_border: f32,
    /// Darkens the action colour when telling whether text on it is drawn light or dark.
    lightness_shift: f32,
    border: Option<u32>,
    secondary_background: Option<u32>,
    selected: Option<u32>,
    selected_text: Option<u32>,
    selected_on_secondary: Option<u32>,
}

/// KeeWeb's defaults.
const BASE: Palette = Palette {
    background: 0,
    medium: 0,
    text: 0,
    action: 0,
    error: 0,
    mute: 0.3,
    light_border: 0.2,
    lightness_shift: 0.,
    border: None,
    secondary_background: None,
    selected: None,
    selected_text: None,
    selected_on_secondary: None,
};

const DARK: Palette = Palette {
    background: 0x1e1e1e,
    medium: 0xb7b7b8,
    text: 0xf7f7f7,
    action: 0x317ef6,
    error: 0xec655a,
    // rgba(68, 68, 69, 0.7) on the background.
    border: Some(0x393939),
    secondary_background: Some(0x2d2d2e),
    selected: Some(0x2463c8),
    selected_on_secondary: Some(0x403f40),
    ..BASE
};

const LIGHT: Palette = Palette {
    background: 0xf6f6f6,
    medium: 0x4c4c4c,
    text: 0x303030,
    action: 0x3063d4,
    error: 0xec655a,
    mute: 0.5,
    border: Some(0xdedede),
    secondary_background: Some(0xefefef),
    selected: Some(0x2366d9),
    selected_text: Some(0xf6f6f6),
    selected_on_secondary: Some(0xd6d6d6),
    ..BASE
};

const SOLARIZED_DARK: Palette = Palette {
    background: 0x002b36,
    medium: 0x93a1a1,
    text: 0x839496,
    action: 0x859900,
    error: 0xdc322f,
    mute: 0.6,
    lightness_shift: 0.5,
    selected_text: Some(0x002b36),
    ..BASE
};

const SOLARIZED_LIGHT: Palette = Palette {
    background: 0xfdf6e3,
    medium: 0x586e75,
    text: 0x657b83,
    action: 0x859900,
    error: 0xdc322f,
    mute: 0.6,
    selected_text: Some(0xfdf6e3),
    ..BASE
};

const DARK_BLUE: Palette = Palette {
    background: 0x282c34,
    medium: 0xabb2bf,
    text: 0xd7dae0,
    action: 0x528bff,
    error: 0xc34034,
    ..BASE
};

const LIGHT_BLUE: Palette = Palette {
    background: 0xe5e8ec,
    medium: 0x383b48,
    text: 0x282c34,
    action: 0x528bff,
    error: 0xc34034,
    mute: 0.5,
    ..BASE
};

const DARK_BROWN: Palette = Palette {
    background: 0x342f2e,
    medium: 0xfed9d8,
    text: 0xffeae9,
    action: 0x2c9957,
    error: 0xfd6d67,
    ..BASE
};

const LIGHT_BROWN: Palette = Palette {
    background: 0xfcf0e8,
    medium: 0x454040,
    text: 0x342f2e,
    action: 0x2c9957,
    error: 0xfd6d67,
    mute: 0.5,
    ..BASE
};

const TERMINAL: Palette = Palette {
    background: 0x222222,
    medium: 0x999999,
    text: 0xeeeeee,
    action: 0x13a453,
    error: 0xc34034,
    ..BASE
};

const TERMINAL_LIGHT: Palette = Palette {
    background: 0xeeeeee,
    medium: 0x444444,
    text: 0x222222,
    action: 0x13a453,
    error: 0xc34034,
    mute: 0.5,
    ..BASE
};

const DARK_CONTRAST: Palette = Palette {
    background: 0x050505,
    medium: 0xfafafa,
    text: 0xfafafa,
    action: 0x1e5db8,
    error: 0xe74859,
    mute: 0.6,
    light_border: 0.5,
    selected: Some(0x1e5db8),
    selected_text: Some(0xfafafa),
    ..BASE
};

const HIGH_CONTRAST: Palette = Palette {
    background: 0xfafafa,
    medium: 0x050505,
    text: 0x050505,
    action: 0x1e5db8,
    error: 0xe74859,
    mute: 0.6,
    light_border: 0.5,
    selected: Some(0x1e5db8),
    selected_text: Some(0xfafafa),
    ..BASE
};

impl Palette {
    /// The colours KeeWeb mixes from the palette (its `_theme-vars.scss`).
    fn theme(&self) -> Theme {
        let background = rgb_const(self.background);
        let medium = rgb_const(self.medium);
        let text = rgb_const(self.text);
        let action = rgb_const(self.action);
        let or_mix = |picked: Option<u32>, weight| {
            picked.map_or_else(|| mix(medium, background, weight), rgb_const)
        };
        let secondary = or_mix(self.secondary_background, 0.1);
        Theme {
            background,
            sidebar: secondary,
            panel: secondary,
            border: or_mix(self.border, self.light_border),
            text,
            muted: mix(medium, background, self.mute),
            selection: self
                .selected
                .map_or_else(|| mix(action, background, 0.85), rgb_const),
            selection_text: self.selected_text.map_or(text, rgb_const),
            sidebar_selection: or_mix(self.selected_on_secondary, 0.3),
            accent: action,
            // Text on the action colour is the background's if that reads better than the text's.
            accent_text: if lightness(action) - self.lightness_shift >= lightness(background) {
                text
            } else {
                background
            },
            error: rgb_const(self.error),
        }
    }
}

/// `weight` of `a` and the rest of `b`, as Sass's `mix`.
fn mix(a: Rgba, b: Rgba, weight: f32) -> Rgba {
    let channel = |a: f32, b: f32| a * weight + b * (1. - weight);
    Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: 1.0,
    }
}

/// The L of HSL.
fn lightness(color: Rgba) -> f32 {
    let max = color.r.max(color.g).max(color.b);
    let min = color.r.min(color.g).min(color.b);
    (max + min) / 2.
}

pub const fn rgb_const(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_the_action_colour_is_whichever_reads() {
        let on_action = |setting| Theme::of(setting, WindowAppearance::Dark).accent_text;
        // Light text on dark themes' blue, and on light themes' too: the blue is darker.
        assert_eq!(on_action(ThemeSetting::Dark), rgb_const(0xf7f7f7));
        assert_eq!(on_action(ThemeSetting::Light), rgb_const(0xf6f6f6));
        // Solarized dark's green is light enough to want dark text.
        assert_eq!(on_action(ThemeSetting::SolarizedDark), rgb_const(0x002b36));
    }

    #[test]
    fn the_system_theme_follows_the_os() {
        let background = |appearance| Theme::of(ThemeSetting::System, appearance).background;
        assert_eq!(background(WindowAppearance::Dark), rgb_const(0x1e1e1e));
        assert_eq!(background(WindowAppearance::Light), rgb_const(0xf6f6f6));
    }
}
