//! The work that runs for as long as the app does, each on a thread of its own: Merging in
//! changes made to Vault files elsewhere, syncing Dropbox Vaults, the idle and OS locks, and
//! favicon downloads.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use keefast_workspace::{DownloadedFavicon, PendingFavicon, Workspace, settled_vault_file_changes};

use crate::core::{AppCore, Failures};

/// How many favicons download at once.
const DOWNLOADS_AT_ONCE: usize = 4;

/// Starts everything that runs in the background.
pub fn start(core: &Arc<AppCore>) {
    // First, so no favicon download the Workspace begins goes unqueued.
    download_favicons(core);
    watch_vault_files(core);
    sync_online_vaults(core);
    lock_when_idle(core);
    lock_with_the_os(core);
}

/// Watches the open Vaults' Vault files, and Merges in changes made elsewhere once each has
/// settled, saving the Merge.
fn watch_vault_files(core: &Arc<AppCore>) {
    let (changes, changed) = mpsc::channel();
    core.update(|workspace| {
        workspace.watch_vault_files(move |vault| {
            let _ = changes.send(vault);
        })
    });
    let core = Arc::clone(core);
    thread::spawn(move || {
        while let Some(vaults) = settled_vault_file_changes(&changed) {
            core.update(|workspace| {
                for vault in vaults {
                    workspace.vault_file_changed(vault);
                }
            });
            core.autosave();
        }
    });
}

/// Checks Dropbox Vaults for changes made elsewhere whenever one is due, Merging them in and
/// uploading saves made offline.
fn sync_online_vaults(core: &Arc<AppCore>) {
    let core = Arc::clone(core);
    thread::spawn(move || {
        loop {
            thread::sleep(core.read(Workspace::sync_wait));
            // Nothing due changes nothing: the UI needn't hear of it.
            if core.read(Workspace::sync_in) != Some(Duration::ZERO) {
                continue;
            }
            let syncs = core.update(Workspace::begin_syncs);
            core.run_saves(syncs, Failures::Quiet);
        }
    });
}

/// Checks every Dropbox Vault now, without waiting for its turn.
pub fn sync_now(core: &Arc<AppCore>) {
    let syncs = core.update(Workspace::sync_now);
    core.run_saves(syncs, Failures::Announce);
}

/// Locks every Vault after the configured inactivity.
fn lock_when_idle(core: &Arc<AppCore>) {
    let core = Arc::clone(core);
    thread::spawn(move || {
        loop {
            thread::sleep(core.read(Workspace::idle_lock_wait));
            if core.read(Workspace::idle_lock_in) != Some(Duration::ZERO) {
                continue;
            }
            core.lock_with(|workspace, clipboard| workspace.lock_if_idle(clipboard));
        }
    });
}

/// Locks every Vault when the OS locks the screen or suspends.
fn lock_with_the_os(core: &Arc<AppCore>) {
    let locking = Arc::clone(core);
    let watched = keefast_workspace::watch_os_lock(move || {
        locking.lock_with(|workspace, clipboard| workspace.os_locked(clipboard));
    });
    if let Err(error) = watched {
        eprintln!("keefast: can't lock when the OS locks or suspends: {error}");
    }
}

/// Downloads the favicons the Workspace wants, a few at a time. Downloads that finish together
/// are handed back together, telling the UI once.
fn download_favicons(core: &Arc<AppCore>) {
    let (queue, pending) = mpsc::channel::<PendingFavicon>();
    let pending = Arc::new(std::sync::Mutex::new(pending));
    let (finished, downloaded) = mpsc::channel::<DownloadedFavicon>();
    for _ in 0..DOWNLOADS_AT_ONCE {
        let pending = Arc::clone(&pending);
        let finished = finished.clone();
        thread::spawn(move || {
            loop {
                let next = pending.lock().map(|pending| pending.recv());
                let Ok(Ok(download)) = next else {
                    return;
                };
                if finished.send(download.run()).is_err() {
                    return;
                }
            }
        });
    }
    let handing_back = Arc::clone(core);
    thread::spawn(move || {
        while let Ok(first) = downloaded.recv() {
            let batch: Vec<_> = std::iter::once(first)
                .chain(downloaded.try_iter())
                .collect();
            handing_back.update(|workspace| {
                for downloaded in batch {
                    workspace.finish_favicon_download(downloaded);
                }
            });
        }
    });
    core.queue_favicons_to(queue);
    // Vaults opened before the threads ran are caught up with now.
    core.update(|_| ());
}
