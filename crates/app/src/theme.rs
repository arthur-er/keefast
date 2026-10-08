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
    pub selection: Rgba,
    pub accent: Rgba,
    pub accent_text: Rgba,
    pub error: Rgba,
}

impl Theme {
    pub fn for_window(window: &Window, cx: &App) -> Self {
        let setting = cx
            .try_global::<ChosenTheme>()
            .map_or(ThemeSetting::System, |chosen| chosen.0);
        match setting {
            ThemeSetting::Light => Self::LIGHT,
            ThemeSetting::Dark => Self::DARK,
            ThemeSetting::System => match window.appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::DARK,
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::LIGHT,
            },
        }
    }

    const DARK: Theme = Theme {
        background: rgb_const(0x1e1f22),
        sidebar: rgb_const(0x18191b),
        panel: rgb_const(0x232427),
        border: rgb_const(0x34363a),
        text: rgb_const(0xe6e6e6),
        muted: rgb_const(0x8b8e94),
        selection: rgb_const(0x2f4a6d),
        accent: rgb_const(0x3d7bd9),
        accent_text: rgb_const(0xffffff),
        error: rgb_const(0xf0707a),
    };

    const LIGHT: Theme = Theme {
        background: rgb_const(0xffffff),
        sidebar: rgb_const(0xf2f3f5),
        panel: rgb_const(0xfafafa),
        border: rgb_const(0xdcdee2),
        text: rgb_const(0x1d1f23),
        muted: rgb_const(0x6b6f76),
        selection: rgb_const(0xd4e3fa),
        accent: rgb_const(0x2b6cd4),
        accent_text: rgb_const(0xffffff),
        error: rgb_const(0xc62f3b),
    };
}

pub const fn rgb_const(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}
