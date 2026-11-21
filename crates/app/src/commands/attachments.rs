//! An Entry's attachments: adding files, previewing, saving to disk, removing.

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use keefast_core::{AttachmentPreview, ImageFormat};
use keefast_workspace::{AttachmentEdit, EntryEdit, read_attachment, write_attachment};
use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use super::entries::{edit_entry, find};
use super::{Core, blocking, home, pick_files, pick_new_file};
use crate::core::AppCore;
use crate::dto::{EntryId, VaultId};
use crate::error::{ApiError, ApiResult, ErrorKind};

/// How much of a text attachment a preview shows.
const PREVIEW_TEXT_LIMIT: usize = 16 * 1024;

/// What adding files did: the names they were attached under (numbered if the Entry had one
/// already), and why the others couldn't be read.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddedAttachments {
    pub added: Vec<String>,
    pub errors: Vec<String>,
}

/// An attachment's contents, as the app can show them.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AttachmentContent {
    /// The start of a text attachment; `cut` when there is more.
    Text { text: String, cut: bool },
    /// An image, as a `data:` URL.
    Image { data_url: String },
    /// Neither text nor an image keefast can show.
    None,
}

/// Asks the user for files and attaches them to the Entry, a name another attachment has getting
/// a number. Keeps the version it replaces in the Entry's history.
#[tauri::command]
#[specta::specta]
pub async fn add_attachments(
    app: AppHandle,
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
) -> ApiResult<AddedAttachments> {
    // Checked first, so the user doesn't pick files for an Entry that is gone.
    core.read(|workspace| find(workspace, vault, &entry).map(|_| ()))?;
    let paths = pick_files(&app, "Attach files").await?;
    if paths.is_empty() {
        return Ok(AddedAttachments {
            added: Vec::new(),
            errors: Vec::new(),
        });
    }
    let read = blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                read_attachment(&path).map_err(|error| {
                    let name = path.file_name().unwrap_or(path.as_os_str());
                    format!("{} could not be read: {error}", name.to_string_lossy())
                })
            })
            .collect::<Vec<_>>()
    })
    .await?;
    let mut errors = Vec::new();
    let mut adds = Vec::new();
    for result in read {
        match result {
            Ok(attachment) => adds.push(AttachmentEdit::Add(attachment)),
            Err(error) => errors.push(error),
        }
    }
    let count = adds.len();
    let edit = EntryEdit {
        attachments: adds,
        ..EntryEdit::default()
    };
    let entry = entry.resolve()?;
    let added = core.edit(|workspace| {
        let vault = vault.resolve_open(workspace)?;
        workspace.edit_entry(vault, entry, None, &edit)?;
        // The attachments added are the Entry's last, under the names they were given.
        let attachments = workspace
            .entry(vault, entry)
            .map(|entry| entry.attachments())
            .unwrap_or_default();
        let first = attachments.len().saturating_sub(count);
        let names = attachments[first..]
            .iter()
            .map(|a| a.name.clone())
            .collect();
        Ok::<_, ApiError>(names)
    })?;
    Ok(AddedAttachments { added, errors })
}

/// Removes an attachment from the Entry, keeping the version it replaces in its history.
#[tauri::command]
#[specta::specta]
pub async fn remove_attachment(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    index: u32,
) -> ApiResult<()> {
    remove_attachment_from(&core, vault, &entry, index)
}

pub(crate) fn remove_attachment_from(
    core: &Arc<AppCore>,
    vault: VaultId,
    entry: &EntryId,
    index: u32,
) -> ApiResult<()> {
    let edit = EntryEdit {
        attachments: vec![AttachmentEdit::Remove(index as usize)],
        ..EntryEdit::default()
    };
    edit_entry(core, vault, entry, None, &edit).map(|_| ())
}

/// An attachment's contents to preview: the start of a text, or an image.
#[tauri::command]
#[specta::specta]
pub async fn preview_attachment(
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    index: u32,
) -> ApiResult<AttachmentContent> {
    core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        let attachment = found
            .attachments()
            .get(index as usize)
            .ok_or_else(ApiError::not_found)?;
        Ok(match attachment.preview() {
            AttachmentPreview::Text(text) => {
                let cut = text.len() > PREVIEW_TEXT_LIMIT;
                let shown = &text[..text.floor_char_boundary(PREVIEW_TEXT_LIMIT)];
                AttachmentContent::Text {
                    text: shown.to_owned(),
                    cut,
                }
            }
            AttachmentPreview::Image(format) => AttachmentContent::Image {
                data_url: format!(
                    "data:{};base64,{}",
                    mime(format),
                    BASE64.encode(attachment.data())
                ),
            },
            AttachmentPreview::None => AttachmentContent::None,
        })
    })
}

fn mime(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Webp => "image/webp",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Svg => "image/svg+xml",
    }
}

/// Asks the user where to save an attachment, and saves it there, replacing what is there all or
/// nothing. Returns where, unless the user cancelled.
#[tauri::command]
#[specta::specta]
pub async fn save_attachment(
    app: AppHandle,
    core: Core<'_>,
    vault: VaultId,
    entry: EntryId,
    index: u32,
) -> ApiResult<Option<String>> {
    let attachment = core.read(|workspace| {
        let (_, found) = find(workspace, vault, &entry)?;
        found
            .attachments()
            .get(index as usize)
            .cloned()
            .ok_or_else(ApiError::not_found)
    })?;
    let Some(path) = pick_new_file(&app, "Save attachment", home(), &attachment.name).await? else {
        return Ok(None);
    };
    let written: PathBuf = path.clone();
    let name = attachment.name.clone();
    blocking(move || write_attachment(&attachment, &written))
        .await?
        .map_err(|error| {
            ApiError::new(ErrorKind::Io, format!("{name} could not be saved."))
                .with_detail(error.to_string())
        })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
