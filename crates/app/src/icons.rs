//! How Entries' icons and colours are drawn: KeePass's standard icons as Lucide icons from
//! gpui-kit's catalog, custom icons from the Vault. The app's own buttons draw gpui-kit's
//! Lucide icons too.

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    AnyElement, AssetSource, Image, ImageFormat, Pixels, Rgba, SharedString, img, prelude::*,
};
use gpui_component::Icon;
pub use gpui_kit_assets::IconName;
use keefast_core::{EntryIcon, STANDARD_ICON_COUNT, Vault};

/// The Lucide icon drawn for each of KeePass's standard icons, by number: KeePass numbers them
/// and each client draws them its own way, so these are the nearest in meaning to KeePass's
/// names (PwIcon), from Key and World to Certificate and BlackBerry.
const STANDARD_ICONS: [IconName; STANDARD_ICON_COUNT as usize] = [
    IconName::Key,
    IconName::Globe,
    IconName::TriangleAlert,
    IconName::Server,
    IconName::FolderCheck,
    IconName::MessagesSquare,
    IconName::Puzzle,
    IconName::NotepadText,
    IconName::Plug,
    IconName::IdCard,
    IconName::FileCheck,
    IconName::Camera,
    IconName::Radio,
    IconName::KeyRound,
    IconName::Zap,
    IconName::Scan,
    IconName::Sparkles,
    IconName::Disc,
    IconName::Monitor,
    IconName::Mail,
    IconName::Settings,
    IconName::ClipboardCheck,
    IconName::FilePlus,
    IconName::Tv,
    IconName::BatteryWarning,
    IconName::Mailbox,
    IconName::Save,
    IconName::HardDrive,
    IconName::FileQuestionMark,
    IconName::SquareTerminal,
    IconName::Terminal,
    IconName::Printer,
    IconName::LayoutGrid,
    IconName::Play,
    IconName::Wrench,
    IconName::Network,
    IconName::Archive,
    IconName::Landmark,
    IconName::AppWindow,
    IconName::Clock,
    IconName::MailSearch,
    IconName::Flag,
    IconName::MemoryStick,
    IconName::Trash,
    IconName::StickyNote,
    IconName::Ban,
    IconName::Info,
    IconName::Package,
    IconName::Folder,
    IconName::FolderOpen,
    IconName::FolderArchive,
    IconName::LockOpen,
    IconName::FileLock,
    IconName::CircleCheck,
    IconName::Pen,
    IconName::Image,
    IconName::Book,
    IconName::List,
    IconName::User,
    IconName::Hammer,
    IconName::House,
    IconName::Star,
    IconName::Bird,
    IconName::Feather,
    IconName::Apple,
    IconName::BookOpen,
    IconName::Banknote,
    IconName::Award,
    IconName::Smartphone,
];

/// The standard icon of a key, KeePass's first.
const KEY_ICON: u32 = 0;

// The icons the app draws beyond those gpui-kit's components draw.
gpui_kit_assets::icon_assets!(
    AppIcons,
    [
        AppWindow,
        Apple,
        Archive,
        ArrowDownWideNarrow,
        Award,
        Ban,
        Banknote,
        BatteryWarning,
        Bird,
        Book,
        BookOpen,
        Bookmark,
        Camera,
        CircleArrowLeft,
        CircleCheck,
        CircleQuestionMark,
        ClipboardCheck,
        Clock,
        Disc,
        Feather,
        FileCheck,
        FileLock,
        FilePlus,
        FileQuestionMark,
        Flag,
        Folder,
        FolderArchive,
        FolderCheck,
        FolderOpen,
        Globe,
        Hammer,
        HardDrive,
        House,
        IdCard,
        Image,
        Info,
        Key,
        KeyRound,
        Landmark,
        LayoutGrid,
        List,
        Lock,
        LockOpen,
        LogOut,
        Mail,
        MailSearch,
        Mailbox,
        MemoryStick,
        MessagesSquare,
        Monitor,
        Network,
        NotepadText,
        Package,
        Pen,
        Pencil,
        Play,
        Plug,
        Plus,
        Printer,
        Puzzle,
        Radio,
        Save,
        Scan,
        Search,
        Server,
        Settings,
        Smartphone,
        Sparkles,
        SquareTerminal,
        Star,
        StickyNote,
        Tags,
        Terminal,
        Trash,
        TriangleAlert,
        Tv,
        User,
        Wrench,
        Zap,
    ]
);

/// One of the app's own icons, `size` square, drawn in `color`.
pub fn ui_icon(icon: IconName, size: Pixels, color: Rgba) -> Icon {
    Icon::new(icon).size(size).flex_none().text_color(color)
}

/// The pictures the app draws, which GPUI loads by path: its own icons, then those of
/// gpui-kit's components.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match AppIcons.load(path)? {
            Some(icon) => Ok(Some(icon)),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = AppIcons.list(path)?;
        paths.extend(gpui_kit_assets::Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
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
    let icon = STANDARD_ICONS[number.min(STANDARD_ICON_COUNT - 1) as usize];
    ui_icon(icon, size, color).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_drawn_has_a_picture() {
        let used = STANDARD_ICONS.into_iter().chain([
            IconName::LayoutGrid,
            IconName::Bookmark,
            IconName::Tags,
            IconName::Trash,
            IconName::Lock,
            IconName::LockOpen,
            IconName::Plus,
            IconName::CircleQuestionMark,
            IconName::Settings,
            IconName::Check,
            IconName::Search,
            IconName::ArrowDownWideNarrow,
            IconName::Pencil,
            IconName::CircleArrowLeft,
        ]);
        for icon in used {
            let svg = Assets.load(&icon.path()).unwrap().expect("bundled");
            assert!(svg.starts_with(b"<svg"), "{icon:?}");
        }
    }
}
