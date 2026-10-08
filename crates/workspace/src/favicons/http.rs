//! Downloading favicons from the sites themselves, never through a third-party favicon service,
//! which would learn every site in the user's Vaults.

use std::io::Cursor;
use std::time::Duration;

use image::{ImageFormat, imageops::FilterType};
use keefast_core::Site;
use ureq::Agent;
use url::Url;

use super::{Favicon, FaviconSource};

/// The size favicons are kept at, in pixels: twice what the app shows, for HiDPI screens.
const SIZE: u32 = 32;
/// The most read of an icon, and of a home page looked through for an icon link.
const ICON_LIMIT: u64 = 512 * 1024;
const PAGE_LIMIT: u64 = 512 * 1024;

/// Asks each site for `/favicon.ico`, and failing that for the icon its home page links to.
pub struct HttpFavicons {
    agent: Agent,
}

impl HttpFavicons {
    pub fn new() -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(5)
            .user_agent(concat!("keefast/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }

    fn get(&self, url: &str, limit: u64) -> Option<Vec<u8>> {
        let mut response = self.agent.get(url).call().ok()?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .ok()
    }

    fn icon_at(&self, url: &str) -> Option<Favicon> {
        to_png(&self.get(url, ICON_LIMIT)?)
    }

    fn linked_icon(&self, origin: &str) -> Option<Favicon> {
        let home = Url::parse(&format!("{origin}/")).ok()?;
        let page = self.get(home.as_str(), PAGE_LIMIT)?;
        let href = icon_link(&String::from_utf8_lossy(&page))?;
        let icon = home.join(&href).ok()?;
        if !matches!(icon.scheme(), "http" | "https") {
            return None;
        }
        self.icon_at(icon.as_str())
    }
}

impl Default for HttpFavicons {
    fn default() -> Self {
        Self::new()
    }
}

impl FaviconSource for HttpFavicons {
    fn download(&self, site: &Site) -> Option<Favicon> {
        let origin = site.origin();
        self.icon_at(&format!("{origin}/favicon.ico"))
            .or_else(|| self.linked_icon(&origin))
    }
}

/// An icon in any format the `image` crate reads (ICO, PNG, GIF, JPEG, BMP, WebP), scaled
/// down to [`SIZE`] and turned into a PNG. `None` if it isn't an image.
fn to_png(bytes: &[u8]) -> Option<Favicon> {
    let mut icon = image::load_from_memory(bytes).ok()?;
    if icon.width() > SIZE || icon.height() > SIZE {
        icon = icon.resize(SIZE, SIZE, FilterType::Lanczos3);
    }
    let mut png = Vec::new();
    icon.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(Favicon::from_png(png))
}

/// Where the first icon a page links to is, as written in its `href`: an `icon` link's, or
/// else an `apple-touch-icon` link's.
fn icon_link(html: &str) -> Option<String> {
    let mut touch_icon = None;
    let lower = html.to_ascii_lowercase();
    let mut rest = 0;
    while let Some(start) = lower[rest..].find("<link") {
        let start = rest + start;
        let end = lower[start..]
            .find('>')
            .map_or(lower.len(), |end| start + end);
        rest = end;
        let tag = &html[start + "<link".len()..end];
        let (Some(rel), Some(href)) = (attribute(tag, "rel"), attribute(tag, "href")) else {
            continue;
        };
        let href = href.replace("&amp;", "&");
        let rel = rel.to_ascii_lowercase();
        if rel.split_ascii_whitespace().any(|token| token == "icon") {
            return Some(href);
        }
        if rel
            .split_ascii_whitespace()
            .any(|token| token == "apple-touch-icon")
        {
            touch_icon.get_or_insert(href);
        }
    }
    touch_icon
}

/// The value of attribute `name` in the attributes of an HTML tag, quoted or not.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let at = from + at;
        from = at + name.len();
        // Part of a longer name, such as `data-href`.
        let before = lower[..at].chars().next_back();
        if before.is_some_and(|c| !c.is_ascii_whitespace() && c != '"' && c != '\'') {
            continue;
        }
        let after = lower[from..].trim_start();
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let offset = tag.len() - value.len();
        let value = &tag[offset..];
        let value = match value.chars().next()? {
            quote @ ('"' | '\'') => {
                let value = &value[1..];
                &value[..value.find(quote)?]
            }
            _ => value.split(|c: char| c.is_ascii_whitespace()).next()?,
        };
        return (!value.is_empty()).then(|| value.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_links_its_icon() {
        let page = r#"<html><head>
            <link rel="stylesheet" href="/site.css">
            <LINK REL="Shortcut Icon" HREF="/static/icon.png?v=1&amp;x=2">
            <link rel="icon" href="/second.png">
        </head></html>"#;
        assert_eq!(icon_link(page).as_deref(), Some("/static/icon.png?v=1&x=2"));
    }

    #[test]
    fn an_apple_touch_icon_is_taken_when_no_icon_is_linked() {
        let page = "<link href='/touch.png' rel='apple-touch-icon'><link rel=icon href=/i.ico>";
        assert_eq!(icon_link(page).as_deref(), Some("/i.ico"));
        let page = "<link data-href=\"/no.png\" href='/touch.png' rel='apple-touch-icon'>";
        assert_eq!(icon_link(page).as_deref(), Some("/touch.png"));
    }

    #[test]
    fn a_page_without_an_icon_link_has_none() {
        assert_eq!(
            icon_link("<link rel=stylesheet href=a.css><p>icon</p>"),
            None
        );
        assert_eq!(icon_link("<link rel=\"icon\">"), None);
        assert_eq!(icon_link("<link rel=\"icon\" href=\""), None);
    }

    #[test]
    fn icons_become_small_pngs() {
        let mut ico = Vec::new();
        image::DynamicImage::new_rgba8(64, 64)
            .write_to(&mut Cursor::new(&mut ico), ImageFormat::Ico)
            .unwrap();

        let png = to_png(&ico).unwrap();

        let icon = image::load_from_memory_with_format(png.png(), ImageFormat::Png).unwrap();
        assert_eq!((icon.width(), icon.height()), (SIZE, SIZE));
        assert_eq!(to_png(b"<html>not an icon</html>"), None);
    }
}
