//! Favicon downloads from the app: started whenever the Workspace changes, as when a Vault
//! unlocks, an Entry changes or favicon download is turned on, and run off the UI thread a few
//! at a time.

use std::sync::Arc;

use gpui::{AnyElement, App, Entity, Image, ImageFormat, ImageSource, img, prelude::*, px};
use keefast_core::EntryId;
use keefast_workspace::{PendingFavicon, VaultId, Workspace};

/// How many favicons download at once.
const DOWNLOADS_AT_ONCE: usize = 4;

/// Downloads the favicons the Workspace wants, for as long as the app runs.
pub fn watch(workspace: &Entity<Workspace>, cx: &mut App) {
    let (queue, pending) = async_channel::unbounded::<PendingFavicon>();
    let (finished, downloaded) = async_channel::unbounded();
    cx.observe(workspace, move |workspace, cx| {
        let downloads = workspace.update(cx, |workspace, _| workspace.begin_favicon_downloads());
        for download in downloads {
            let _ = queue.send_blocking(download);
        }
    })
    .detach();
    for _ in 0..DOWNLOADS_AT_ONCE {
        let pending = pending.clone();
        let finished = finished.clone();
        cx.spawn(async move |cx| {
            while let Ok(download) = pending.recv().await {
                let downloaded = cx
                    .background_executor()
                    .spawn(async move { download.run() })
                    .await;
                if finished.send(downloaded).await.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
    // Downloads that finish together are handed back together, redrawing once.
    let workspace = workspace.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(first) = downloaded.recv().await {
            let mut batch = vec![first];
            while let Ok(more) = downloaded.try_recv() {
                batch.push(more);
            }
            let kept = workspace.update(cx, |workspace, cx| {
                for downloaded in batch {
                    workspace.finish_favicon_download(downloaded);
                }
                cx.notify();
            });
            if kept.is_err() {
                return;
            }
        }
    })
    .detach();
}

/// An Entry's favicon, `size` pixels square, once downloaded, with favicon download on.
pub fn image(
    workspace: &Workspace,
    vault: VaultId,
    entry: EntryId,
    size: f32,
) -> Option<AnyElement> {
    if !workspace.downloads_favicons() {
        return None;
    }
    let favicon = workspace.favicon(vault, entry)?;
    let image = Image::from_bytes(ImageFormat::Png, favicon.png().to_vec());
    Some(
        img(ImageSource::Image(Arc::new(image)))
            .flex_none()
            .size(px(size))
            .into_any_element(),
    )
}
