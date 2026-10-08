//! Locking from the app: the global Lock, one Vault's, and the idle and OS locks, with the
//! Vaults to save first saved off the UI thread.

use std::time::Duration;

use gpui::{App, Entity};
use keefast_workspace::{PendingSave, UnsavedLock, VaultId, Workspace};

use crate::clipboard::AppClipboard;
use crate::saving;

/// Locks every Vault.
pub fn lock_all(workspace: &Entity<Workspace>, cx: &mut App) {
    lock_with(workspace, cx, |workspace, clipboard| {
        workspace.lock_all(clipboard)
    });
}

pub fn lock_vault(workspace: &Entity<Workspace>, vault: VaultId, cx: &mut App) {
    lock_with(workspace, cx, |workspace, clipboard| {
        workspace.lock_vault(vault, clipboard)
    });
}

/// Answers whether to save a Vault's unsaved changes before it locks.
pub fn answer(workspace: &Entity<Workspace>, vault: VaultId, answer: UnsavedLock, cx: &mut App) {
    let save = workspace.update(cx, |workspace, cx| {
        cx.notify();
        workspace.answer_lock(vault, answer)
    });
    saving::run(workspace, save.into_iter().collect(), cx);
}

/// Tells the Workspace the user gave keefast input.
pub fn note_activity(workspace: &Entity<Workspace>, cx: &mut App) {
    workspace.update(cx, |workspace, _| workspace.note_activity());
}

/// Locks every Vault after the configured inactivity, and when the OS locks or suspends, for
/// as long as the app runs.
pub fn watch(workspace: &Entity<Workspace>, cx: &mut App) {
    let idle = workspace.clone();
    cx.spawn(async move |cx| {
        // With no idle lock due, look again now and then: a Vault may be unlocked meanwhile.
        const AT_MOST: Duration = Duration::from_secs(60);
        while let Ok(wait) = idle.read_with(cx, |workspace, _| workspace.idle_lock_in()) {
            cx.background_executor()
                .timer(wait.unwrap_or(AT_MOST).min(AT_MOST))
                .await;
            let locked = cx.update(|cx| {
                lock_with(&idle, cx, |workspace, clipboard| {
                    workspace.lock_if_idle(clipboard)
                })
            });
            if locked.is_err() {
                return;
            }
        }
    })
    .detach();

    let (events, os_locks) = async_channel::unbounded();
    let watched = keefast_workspace::watch_os_lock(move || {
        let _ = events.send_blocking(());
    });
    if let Err(error) = watched {
        eprintln!("keefast: can't lock when the OS locks or suspends: {error}");
        return;
    }
    let workspace = workspace.clone();
    cx.spawn(async move |cx| {
        while os_locks.recv().await.is_ok() {
            let locked = cx.update(|cx| {
                lock_with(&workspace, cx, |workspace, clipboard| {
                    workspace.os_locked(clipboard)
                })
            });
            if locked.is_err() {
                return;
            }
        }
    })
    .detach();
}

fn lock_with(
    workspace: &Entity<Workspace>,
    cx: &mut App,
    lock: impl FnOnce(&mut Workspace, &mut AppClipboard) -> Vec<PendingSave>,
) {
    let saves = workspace.update(cx, |workspace, cx| {
        let saves = lock(workspace, &mut AppClipboard(cx));
        cx.notify();
        saves
    });
    saving::run(workspace, saves, cx);
}
