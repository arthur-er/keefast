//! How Entries' icons and colours are drawn: KeePass's standard icons as pictures bundled with
//! the app (from Lucide, see `assets/icons/standard/LICENSE`), custom icons from the Vault. The
//! app's own buttons draw Lucide icons too (`assets/icons/ui`).

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    AnyElement, AssetSource, Image, ImageFormat, Pixels, Rgba, SharedString, div, img, prelude::*,
    svg,
};
use keefast_core::{EntryColor, EntryIcon, STANDARD_ICON_COUNT, Vault};

use crate::theme::rgb_const;

macro_rules! standard_icons {
    ($($number:literal)*) => {
        [$(include_bytes!(concat!("../assets/icons/standard/", $number, ".svg")) as &[u8]),*]
    };
}

/// The picture for each standard icon, by number.
const STANDARD_ICONS: [&[u8]; STANDARD_ICON_COUNT as usize] = standard_icons!(
    "00" "01" "02" "03" "04" "05" "06" "07" "08" "09" "10" "11" "12" "13" "14" "15" "16" "17"
    "18" "19" "20" "21" "22" "23" "24" "25" "26" "27" "28" "29" "30" "31" "32" "33" "34" "35"
    "36" "37" "38" "39" "40" "41" "42" "43" "44" "45" "46" "47" "48" "49" "50" "51" "52" "53"
    "54" "55" "56" "57" "58" "59" "60" "61" "62" "63" "64" "65" "66" "67" "68"
);

const STANDARD_PREFIX: &str = "icons/standard/";

/// The standard icon of a key, KeePass's first.
const KEY_ICON: u32 = 0;

/// The icons of the app's own buttons and sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiIcon {
    AllItems,
    Colors,
    Tags,
    Trash,
    Lock,
    Unlocked,
    Open,
    Help,
    Settings,
    LockAll,
    Search,
    Sort,
    Edit,
    Back,
}

impl UiIcon {
    pub const ALL: [UiIcon; 14] = [
        Self::AllItems,
        Self::Colors,
        Self::Tags,
        Self::Trash,
        Self::Lock,
        Self::Unlocked,
        Self::Open,
        Self::Help,
        Self::Settings,
        Self::LockAll,
        Self::Search,
        Self::Sort,
        Self::Edit,
        Self::Back,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::AllItems => "layout-grid",
            Self::Colors => "bookmark",
            Self::Tags => "tags",
            Self::Trash => "trash",
            Self::Lock => "lock",
            Self::Unlocked => "lock-open",
            Self::Open => "plus",
            Self::Help => "help",
            Self::Settings => "settings",
            Self::LockAll => "log-out",
            Self::Search => "search",
            Self::Sort => "arrow-down-wide-narrow",
            Self::Edit => "pencil",
            Self::Back => "circle-arrow-left",
        }
    }

    fn svg(self) -> &'static [u8] {
        match self {
            Self::AllItems => include_bytes!("../assets/icons/ui/layout-grid.svg"),
            Self::Colors => include_bytes!("../assets/icons/ui/bookmark.svg"),
            Self::Tags => include_bytes!("../assets/icons/ui/tags.svg"),
            Self::Trash => include_bytes!("../assets/icons/ui/trash.svg"),
            Self::Lock => include_bytes!("../assets/icons/ui/lock.svg"),
            Self::Unlocked => include_bytes!("../assets/icons/ui/lock-open.svg"),
            Self::Open => include_bytes!("../assets/icons/ui/plus.svg"),
            Self::Help => include_bytes!("../assets/icons/ui/help.svg"),
            Self::Settings => include_bytes!("../assets/icons/ui/settings.svg"),
            Self::LockAll => include_bytes!("../assets/icons/ui/log-out.svg"),
            Self::Search => include_bytes!("../assets/icons/ui/search.svg"),
            Self::Sort => include_bytes!("../assets/icons/ui/arrow-down-wide-narrow.svg"),
            Self::Edit => include_bytes!("../assets/icons/ui/pencil.svg"),
            Self::Back => include_bytes!("../assets/icons/ui/circle-arrow-left.svg"),
        }
    }

    fn path(self) -> SharedString {
        format!("{UI_PREFIX}{}.svg", self.name()).into()
    }
}

const UI_PREFIX: &str = "icons/ui/";

/// One of the app's own icons, `size` square, drawn in `color`.
pub fn ui_icon(icon: UiIcon, size: Pixels, color: Rgba) -> gpui::Svg {
    svg()
        .path(icon.path())
        .size(size)
        .flex_none()
        .text_color(color)
}

/// The pictures the app draws, which GPUI loads by path.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path.starts_with(UI_PREFIX) {
            let icon = UiIcon::ALL.into_iter().find(|icon| icon.path() == path);
            return Ok(icon.map(|icon| Cow::Borrowed(icon.svg())));
        }
        let icon = path
            .strip_prefix(STANDARD_PREFIX)
            .and_then(|name| name.strip_suffix(".svg"))
            .and_then(|number| number.parse::<usize>().ok())
            .and_then(|number| STANDARD_ICONS.get(number));
        Ok(icon.map(|svg| Cow::Borrowed(*svg)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok((0..STANDARD_ICON_COUNT)
            .map(standard_icon_path)
            .chain(UiIcon::ALL.into_iter().map(UiIcon::path))
            .filter(|icon| icon.starts_with(path))
            .collect())
    }
}

fn standard_icon_path(number: u32) -> SharedString {
    format!("{STANDARD_PREFIX}{number:02}.svg").into()
}

/// An Entry's icon, `size` square: its custom icon from `vault`, or its standard icon drawn
/// in `color`. A custom icon the Vault hasn't got shows as the key.
pub fn entry_icon(icon: EntryIcon, vault: &Vault, size: Pixels, color: Rgba) -> AnyElement {
    let standard = match icon {
        EntryIcon::Standard(number) => number,
        EntryIcon::Custom(id) => match vault.custom_icon(id) {
            Some(custom) => {
                let image = Image::from_bytes(ImageFormat::Png, custom.data().to_vec());
                return img(Arc::new(image))
                    .size(size)
                    .flex_none()
                    .into_any_element();
            }
            None => KEY_ICON,
        },
    };
    standard_icon(standard, size, color)
}

/// A standard icon, `size` square, drawn in `color`.
pub fn standard_icon(number: u32, size: Pixels, color: Rgba) -> AnyElement {
    svg()
        .path(standard_icon_path(number.min(STANDARD_ICON_COUNT - 1)))
        .size(size)
        .flex_none()
        .text_color(color)
        .into_any_element()
}

/// How the app draws each colour: brighter than the Vault file's, to stand out as a dot.
pub fn color_rgba(color: EntryColor) -> Rgba {
    match color {
        EntryColor::Red => rgb_const(0xe5484d),
        EntryColor::Orange => rgb_const(0xf76b15),
        EntryColor::Yellow => rgb_const(0xf5c518),
        EntryColor::Green => rgb_const(0x30a46c),
        EntryColor::Blue => rgb_const(0x3e83f8),
        EntryColor::Violet => rgb_const(0x8e4ec6),
    }
}

/// A dot in an Entry's colour.
pub fn color_dot(color: EntryColor, size: Pixels) -> gpui::Div {
    div()
        .size(size)
        .flex_none()
        .rounded_full()
        .bg(color_rgba(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_standard_icon_has_a_picture() {
        for number in 0..STANDARD_ICON_COUNT {
            let path = standard_icon_path(number);
            let svg = Assets.load(&path).unwrap().expect("bundled");
            assert!(svg.starts_with(b"<svg"), "{path}");
        }
        assert!(Assets.load("icons/standard/69.svg").unwrap().is_none());
        assert_eq!(
            Assets.list("icons/standard/").unwrap().len(),
            STANDARD_ICON_COUNT as usize
        );
    }

    #[test]
    fn every_ui_icon_has_a_picture() {
        for icon in UiIcon::ALL {
            let svg = Assets.load(&icon.path()).unwrap().expect("bundled");
            assert!(svg.starts_with(b"<svg"), "{icon:?}");
        }
        assert_eq!(Assets.list("icons/ui/").unwrap().len(), UiIcon::ALL.len());
    }
}
