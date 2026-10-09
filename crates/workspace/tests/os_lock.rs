//! Hearing the OS lock on Linux: logind and a screen saver faked on private D-Bus buses
//! standing in for the system and session buses. Skipped where `dbus-daemon` isn't installed.

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

const OWN_SESSION: &str = "/org/freedesktop/login1/session/_1";

/// A private message bus, stopped when dropped.
struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start() -> Option<Self> {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut address = String::new();
        BufReader::new(daemon.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        Some(Self {
            daemon,
            address: address.trim().to_owned(),
        })
    }

    /// A connection owning the well-known `name`, serving `at` with `object`.
    fn serve(
        &self,
        name: &str,
        at: &str,
        object: impl zbus::object_server::Interface,
    ) -> Connection {
        Builder::address(self.address.as_str())
            .unwrap()
            .name(name.to_owned())
            .unwrap()
            .serve_at(at.to_owned(), object)
            .unwrap()
            .build()
            .unwrap()
    }

    fn own(&self, name: &str) -> Connection {
        Builder::address(self.address.as_str())
            .unwrap()
            .name(name.to_owned())
            .unwrap()
            .build()
            .unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// logind's manager, enough to tell the app which session it runs in.
struct Manager;

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, _pid: u32) -> OwnedObjectPath {
        ObjectPath::try_from(OWN_SESSION).unwrap().into()
    }
}

fn emit<B>(bus: &Connection, path: &str, interface: &str, member: &str, body: &B)
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    bus.emit_signal(None::<&str>, path, interface, member, body)
        .unwrap();
}

/// Whether the app heard a lock soon after.
fn heard(locks: &Receiver<()>) -> bool {
    locks.recv_timeout(Duration::from_millis(500)).is_ok()
}

#[test]
fn logind_and_screen_savers_locking_are_heard() {
    let (Some(system), Some(session)) = (Bus::start(), Bus::start()) else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let logind = system.serve("org.freedesktop.login1", "/org/freedesktop/login1", Manager);
    let gnome = session.own("org.gnome.ScreenSaver");
    let (sender, locks) = mpsc::channel();
    keefast_workspace::watch_os_lock_on(&system.address, &session.address, move || {
        let _ = sender.send(());
    })
    .unwrap();

    let manager = "org.freedesktop.login1.Manager";
    emit(
        &logind,
        "/org/freedesktop/login1",
        manager,
        "PrepareForSleep",
        &(false,),
    );
    assert!(!heard(&locks), "waking up is not locking");
    emit(
        &logind,
        "/org/freedesktop/login1",
        manager,
        "PrepareForSleep",
        &(true,),
    );
    assert!(heard(&locks), "suspending locks");

    let session_interface = "org.freedesktop.login1.Session";
    emit(
        &logind,
        "/org/freedesktop/login1/session/_2",
        session_interface,
        "Lock",
        &(),
    );
    assert!(!heard(&locks), "another session locking is not this one");
    emit(&logind, OWN_SESSION, session_interface, "Lock", &());
    assert!(heard(&locks), "this session locking locks");

    let saver = "org.gnome.ScreenSaver";
    emit(
        &gnome,
        "/org/gnome/ScreenSaver",
        saver,
        "ActiveChanged",
        &(true,),
    );
    assert!(heard(&locks), "the screen saver locking locks");
    emit(
        &gnome,
        "/org/gnome/ScreenSaver",
        saver,
        "ActiveChanged",
        &(false,),
    );
    assert!(!heard(&locks), "the screen saver unlocking is not locking");

    // Only the real logind is believed, not anyone saying the same.
    let impostor = system.own("org.example.Impostor");
    emit(
        &impostor,
        "/org/freedesktop/login1",
        manager,
        "PrepareForSleep",
        &(true,),
    );
    assert!(!heard(&locks), "an impostor is not logind");
}
