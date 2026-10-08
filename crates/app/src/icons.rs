//! How Entries' icons and colours are drawn: KeePass's standard icons as pictures bundled with
//! the app (from Lucide, see `assets/icons/standard/LICENSE`), custom icons from the Vault.

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

/// The pictures the app draws, which GPUI loads by path.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
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
            Assets.list("icons/").unwrap().len(),
            STANDARD_ICON_COUNT as usize
        );
    }
}
