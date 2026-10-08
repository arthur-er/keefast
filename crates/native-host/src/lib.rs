//! The native-messaging host: the small program Chromium launches for the browser extension.
//! It relays KeePassXC-Browser protocol messages between the browser (stdio) and the running
//! keefast app (local IPC). It never launches or unlocks the app (ADR 0001).

pub mod ipc;
pub mod relay;
