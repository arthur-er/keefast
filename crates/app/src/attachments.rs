//! What the detail pane and the Entry editor share for attachments: sizes, previews, and the
//! file dialogs that read attachments from disk and save them back.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, App, Image, ObjectFit, PathPromptOptions, Task, div, img, prelude::*, px};
use keefast_core::{Attachment, AttachmentPreview, ImageFormat};
use keefast_workspace::{read_attachment, write_attachment};

use crate::strings;
use crate::theme::Theme;

/// How much of a text attachment a preview shows.
const PREVIEW_TEXT_LIMIT: usize = 16 * 1024;

/// A size in bytes for people: `512 B`, `1.5 KB`, `2.3 MB`.
pub fn size_label(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let size = bytes as f64;
    if size < KB {
        strings::SIZE_BYTES.replace("{n}", &bytes.to_string())
    } else if size < KB * KB {
        strings::SIZE_KB.replace("{n}", &format!("{:.1}", size / KB))
    } else {
        strings::SIZE_MB.replace("{n}", &format!("{:.1}", size / (KB * KB)))
    }
}

/// Whether the attachment is text or an image keefast can show.
pub fn can_preview(attachment: &Attachment) -> bool {
    attachment.preview() != AttachmentPreview::None
}

/// An attachment being previewed. An image is decoded once, not on every frame.
pub struct Preview {
    index: usize,
    name: String,
    image: Option<Arc<Image>>,
}

impl Preview {
    pub fn new(index: usize, attachment: &Attachment) -> Self {
        let image = match attachment.preview() {
            AttachmentPreview::Image(format) => Some(Arc::new(Image::from_bytes(
                image_format(format),
                attachment.data().to_vec(),
            ))),
            _ => None,
        };
        Self {
            index,
            name: attachment.name.clone(),
            image,
        }
    }

    /// Whether this previews the attachment at `index`, still the one it was made for.
    pub fn shows(&self, index: usize, attachment: &Attachment) -> bool {
        self.index == index && self.name == attachment.name
    }

    pub fn render(&self, attachment: &Attachment, theme: Theme) -> AnyElement {
        let frame = div()
            .mt_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background);
        if let Some(image) = &self.image {
            return frame
                .child(
                    img(image.clone())
                        .max_w_full()
                        .max_h(px(320.))
                        .object_fit(ObjectFit::ScaleDown),
                )
                .into_any_element();
        }
        let AttachmentPreview::Text(text) = attachment.preview() else {
            return frame.into_any_element();
        };
        let cut = text.len() > PREVIEW_TEXT_LIMIT;
        let shown = if cut {
            &text[..text.floor_char_boundary(PREVIEW_TEXT_LIMIT)]
        } else {
            text
        };
        frame
            .id("attachment-text-preview")
            .max_h(px(320.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .font_family("monospace")
                    .text_sm()
                    .whitespace_normal()
                    .child(shown.to_owned()),
            )
            .when(cut, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted)
                        .child(strings::ATTACHMENT_PREVIEW_CUT),
                )
            })
            .into_any_element()
    }
}

fn image_format(format: ImageFormat) -> gpui::ImageFormat {
    match format {
        ImageFormat::Png => gpui::ImageFormat::Png,
        ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        ImageFormat::Gif => gpui::ImageFormat::Gif,
        ImageFormat::Webp => gpui::ImageFormat::Webp,
        ImageFormat::Bmp => gpui::ImageFormat::Bmp,
        ImageFormat::Svg => gpui::ImageFormat::Svg,
    }
}

/// Asks for files to attach and reads them off the UI thread. Each result is a file read, or
/// what to tell the user about one that couldn't be; `None` when the user picked nothing.
pub fn pick_and_read(cx: &App) -> Task<Option<Vec<Result<Attachment, String>>>> {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: true,
        prompt: Some(strings::ADD_ATTACHMENT_PROMPT.into()),
    });
    let executor = cx.background_executor().clone();
    cx.foreground_executor().spawn(async move {
        let paths = match paths.await {
            Ok(Ok(Some(paths))) => paths,
            Ok(Ok(None)) | Err(_) => return None,
            Ok(Err(_)) => return Some(vec![Err(strings::ERROR_FILE_PICKER.to_owned())]),
        };
        let read = executor.spawn(async move {
            paths
                .iter()
                .map(|path| {
                    read_attachment(path).map_err(|error| {
                        strings::ERROR_ATTACHMENT_READ
                            .replace("{name}", &path.display().to_string())
                            .replace("{reason}", &error.to_string())
                    })
                })
                .collect()
        });
        Some(read.await)
    })
}

/// Asks where to save an attachment, suggesting its name, and saves it there off the UI thread.
/// Resolves to what to tell the user, or `None` when the user cancelled.
pub fn save_to_disk(attachment: Attachment, cx: &App) -> Task<Option<Result<String, String>>> {
    let directory = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let path = cx.prompt_for_new_path(&directory, Some(&attachment.name));
    let executor = cx.background_executor().clone();
    cx.foreground_executor().spawn(async move {
        let path = match path.await {
            Ok(Ok(Some(path))) => path,
            Ok(Ok(None)) | Err(_) => return None,
            Ok(Err(_)) => return Some(Err(strings::ERROR_FILE_PICKER.to_owned())),
        };
        let name = attachment.name.clone();
        let written = executor
            .spawn(async move { write_attachment(&attachment, &path).map(|()| path) })
            .await;
        Some(match written {
            Ok(path) => Ok(strings::ATTACHMENT_SAVED
                .replace("{name}", &name)
                .replace("{path}", &path.display().to_string())),
            Err(error) => Err(strings::ERROR_ATTACHMENT_SAVE
                .replace("{name}", &name)
                .replace("{reason}", &error.to_string())),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_the_largest_fitting_unit() {
        assert_eq!(size_label(6), "6 B");
        assert_eq!(size_label(1023), "1023 B");
        assert_eq!(size_label(1536), "1.5 KB");
        assert_eq!(size_label(5 * 1024 * 1024 / 2), "2.5 MB");
    }
}
