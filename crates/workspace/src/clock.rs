use std::time::SystemTime;

/// Tells the Workspace the time, for the timestamps edits record. Injected so tests control it.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// The real time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
