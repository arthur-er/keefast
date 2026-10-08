//! Hearing the OS lock the screen or suspend the machine, so the Vaults can lock with it
//! (ADR 0004). On Linux that is logind's sleep and session-lock signals and the screen
//! savers' on the D-Bus; on Windows, the session-lock and suspend window messages.

use std::io;

/// Calls `on_lock` from a background thread whenever the OS screen locks or the machine is
/// about to suspend, for as long as the app runs. Fails if nothing can be listened to.
pub fn watch_os_lock(on_lock: impl Fn() + Send + Sync + 'static) -> io::Result<()> {
    platform::watch(Box::new(on_lock))
}

type OnLock = Box<dyn Fn() + Send + Sync>;

#[cfg(target_os = "linux")]
pub use platform::watch_os_lock_on;

#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::sync::Arc;
    use std::thread;

    use zbus::blocking::fdo::DBusProxy;
    use zbus::blocking::{Connection, MessageIterator};
    use zbus::message::Type;
    use zbus::zvariant::OwnedObjectPath;
    use zbus::{MatchRule, Message};

    use super::OnLock;

    const LOGIND: &str = "org.freedesktop.login1";
    const LOGIND_PATH: &str = "/org/freedesktop/login1";
    const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";
    const LOGIND_SESSION: &str = "org.freedesktop.login1.Session";
    /// The screen savers that announce locking with `ActiveChanged(true)`: KDE's and the
    /// freedesktop one, GNOME's, and Xfce's.
    const SCREEN_SAVERS: [&str; 3] = [
        "org.freedesktop.ScreenSaver",
        "org.gnome.ScreenSaver",
        "org.xfce.ScreenSaver",
    ];

    pub fn watch(on_lock: OnLock) -> io::Result<()> {
        let system = Connection::system().map_err(io::Error::other);
        let session = Connection::session().map_err(io::Error::other);
        watch_buses(system, session, on_lock)
    }

    /// [`super::watch_os_lock`] on the buses at the given D-Bus addresses, as stand-ins for
    /// the system and session buses.
    pub fn watch_os_lock_on(
        system: &str,
        session: &str,
        on_lock: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<()> {
        let connect = |address: &str| {
            zbus::blocking::connection::Builder::address(address)
                .and_then(|builder| builder.build())
                .map_err(io::Error::other)
        };
        watch_buses(connect(system), connect(session), Box::new(on_lock))
    }

    fn watch_buses(
        system: io::Result<Connection>,
        session: io::Result<Connection>,
        on_lock: OnLock,
    ) -> io::Result<()> {
        let on_lock: Arc<OnLock> = Arc::new(on_lock);
        let system = system.and_then(|bus| {
            let mut rules = vec![signal(LOGIND, LOGIND_MANAGER, "PrepareForSleep", None)];
            // Only this session's locking counts, not another user's.
            if let Some(path) = own_session(&bus) {
                rules.push(signal(LOGIND, LOGIND_SESSION, "Lock", Some(path)));
            }
            listen(bus, rules, Arc::clone(&on_lock))
        });
        let session = session.and_then(|bus| {
            let rules = SCREEN_SAVERS
                .iter()
                .map(|saver| signal(saver, saver, "ActiveChanged", None))
                .collect();
            listen(bus, rules, on_lock)
        });
        // Either bus is enough: a desktop without logind may still have a screen saver.
        system.or(session)
    }

    fn signal(
        sender: &'static str,
        interface: &'static str,
        member: &'static str,
        path: Option<OwnedObjectPath>,
    ) -> io::Result<MatchRule<'static>> {
        let mut rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(sender)
            .and_then(|rule| rule.interface(interface))
            .and_then(|rule| rule.member(member))
            .map_err(io::Error::other)?;
        if let Some(path) = path {
            rule = rule.path(path).map_err(io::Error::other)?;
        }
        Ok(rule.build())
    }

    /// Listens on `bus` for the signals the rules match, on a thread of its own. The bus only
    /// passes on signals matching the rules, which name their senders and paths.
    fn listen(
        bus: Connection,
        rules: Vec<io::Result<MatchRule<'static>>>,
        on_lock: Arc<OnLock>,
    ) -> io::Result<()> {
        let rules: Vec<MatchRule<'static>> = rules.into_iter().collect::<io::Result<_>>()?;
        let messages = MessageIterator::from(&bus);
        let proxy = DBusProxy::new(&bus).map_err(io::Error::other)?;
        for rule in &rules {
            proxy
                .add_match_rule(rule.clone())
                .map_err(io::Error::other)?;
        }
        thread::Builder::new()
            .name("os-lock".into())
            .spawn(move || {
                for message in messages.flatten() {
                    let header = message.header();
                    let signal = (header.interface(), header.member());
                    let matched = rules.iter().any(|rule| {
                        header.message_type() == Type::Signal
                            && (rule.interface(), rule.member()) == signal
                    });
                    if matched && locks(&message) {
                        on_lock();
                    }
                }
            })?;
        Ok(())
    }

    /// Whether the signal says the machine is about to sleep or the screen is locking:
    /// `PrepareForSleep(true)`, `ActiveChanged(true)` or a session's `Lock`.
    fn locks(message: &Message) -> bool {
        let header = message.header();
        match header.member().map(|member| member.as_str()) {
            Some("Lock") => true,
            Some(_) => message.body().deserialize::<bool>().unwrap_or(false),
            None => false,
        }
    }

    /// The logind session the app runs in, if logind knows it.
    fn own_session(bus: &Connection) -> Option<OwnedObjectPath> {
        let by_pid = bus.call_method(
            Some(LOGIND),
            LOGIND_PATH,
            Some(LOGIND_MANAGER),
            "GetSessionByPID",
            &(std::process::id()),
        );
        let by_id = || {
            let id = std::env::var("XDG_SESSION_ID").ok()?;
            bus.call_method(
                Some(LOGIND),
                LOGIND_PATH,
                Some(LOGIND_MANAGER),
                "GetSession",
                &(id),
            )
            .ok()
        };
        let reply = by_pid.ok().or_else(by_id)?;
        reply.body().deserialize().ok()
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::ptr;
    use std::sync::OnceLock;
    use std::thread;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMSUSPEND,
        RegisterClassW, TranslateMessage, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
        WTS_SESSION_LOCK,
    };

    use super::OnLock;

    static ON_LOCK: OnceLock<OnLock> = OnceLock::new();

    /// Listens with a hidden window of its own, on a thread of its own: Windows tells windows
    /// about session locks once registered for them, and every top-level window about suspend.
    pub fn watch(on_lock: OnLock) -> io::Result<()> {
        if ON_LOCK.set(on_lock).is_err() {
            return Err(io::Error::other("already watching the OS lock"));
        }
        let (started, result) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("os-lock".into())
            .spawn(move || {
                let window = match create_window() {
                    Ok(window) => window,
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                let _ = started.send(Ok(()));
                pump_messages(window);
            })?;
        result
            .recv()
            .unwrap_or_else(|_| Err(io::Error::other("the OS lock watcher stopped")))
    }

    fn create_window() -> io::Result<HWND> {
        let class: Vec<u16> = "keefast-os-lock\0".encode_utf16().collect();
        // SAFETY: the class name outlives the calls, and the window procedure has the right
        // signature. The window is never shown.
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let mut wndclass: WNDCLASSW = std::mem::zeroed();
            wndclass.lpfnWndProc = Some(window_proc);
            wndclass.hInstance = instance;
            wndclass.lpszClassName = class.as_ptr();
            if RegisterClassW(&wndclass) == 0 {
                return Err(io::Error::last_os_error());
            }
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if window.is_null() {
                return Err(io::Error::last_os_error());
            }
            if WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(window)
        }
    }

    fn pump_messages(window: HWND) {
        // SAFETY: `message` is a valid MSG for the calls to fill and read.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, window, 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let locks = match message {
            WM_WTSSESSION_CHANGE => wparam as u32 == WTS_SESSION_LOCK,
            WM_POWERBROADCAST => wparam as u32 == PBT_APMSUSPEND,
            _ => false,
        };
        if locks && let Some(on_lock) = ON_LOCK.get() {
            on_lock();
        }
        // SAFETY: passes the message on as Windows gave it.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use std::io;

    use super::OnLock;

    pub fn watch(_: OnLock) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
