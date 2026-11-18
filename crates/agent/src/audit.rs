//! The audit log: a record of every list and use of a Vault's secrets by Agents and Gateways
//! (ADR 0012 "Audit", ADR 0015).
//!
//! Each record has the time, the pairing, the Entry, the program, or the method and host of an
//! `http_request`, a Gateway's unverified claim of whom it was for, and the outcome, never a
//! value. [`AgentSession`](crate::AgentSession) and
//! [`GatewaySession`](crate::gateway::GatewaySession) write them to the [`AuditSink`] their host
//! gives them; the Headless host's is [`AuditLog`], an append-only file in its state directory,
//! rotated by size, that `keefast-agent log` reads.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::{AgentPairing, PairingKind};

/// What happened when an Agent asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allowed,
    Denied,
}

/// What an Agent did, or asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// `list_secrets`.
    List,
    /// `keefast-agent run`: secrets handed to a program.
    Use,
    HttpRequest,
    /// `create_entry`.
    Create,
    /// `keefast-agent store`.
    Store,
    /// `reveal_secret`.
    Reveal,
    /// A Gateway's resolve: a value handed to another program's vault.
    Resolve,
    /// A Gateway's `POST /logins`.
    CreateLogin,
    /// A Gateway's `PATCH /logins/{ref}`.
    UpdateLogin,
    /// One a later keefast logs and this one doesn't know.
    #[serde(other)]
    Other,
}

impl AuditAction {
    /// Its name in the log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Use => "use",
            Self::HttpRequest => "http_request",
            Self::Create => "create",
            Self::Store => "store",
            Self::Reveal => "reveal",
            Self::Resolve => "resolve",
            Self::CreateLogin => "create_login",
            Self::UpdateLogin => "update_login",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The Entry a record is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub uuid: String,
    pub title: String,
}

/// Whom a Gateway says a call was for: the agent, person and run it names, as free text. It is
/// the Gateway's claim, which keefast can't verify, so the log keeps it apart as `claimed_for`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedFor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

impl ClaimedFor {
    /// The longest each part is logged, in characters; the rest is cut off.
    pub const MAX_CHARS: usize = 256;

    /// This claim as the log keeps it: each part cut to [`Self::MAX_CHARS`], on one line, and
    /// `None` when it names nothing. A control character, such as a line break, reads as a
    /// space, so that a claim can't pass for another line of the log.
    pub fn capped(&self) -> Option<Self> {
        let cap = |part: &Option<String>| {
            part.as_deref().filter(|text| !text.is_empty()).map(|text| {
                text.chars()
                    .take(Self::MAX_CHARS)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect::<String>()
            })
        };
        let capped = Self {
            agent: cap(&self.agent),
            person: cap(&self.person),
            run: cap(&self.run),
        };
        (capped != Self::default()).then_some(capped)
    }
}

/// One thing an Agent did, or was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// When, in seconds since the Unix epoch, by the Workspace's clock.
    pub time: i64,
    /// The Agent pairing's name, as it was then.
    pub pairing: String,
    pub pairing_id: String,
    /// Whether an Agent or a Gateway pairing; records written before Gateways read as Agents'.
    #[serde(default)]
    pub kind: PairingKind,
    pub action: AuditAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<AuditEntry>,
    /// The program a `use` was for, by its executable's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The method of an `http_request`, such as `GET`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// The host an `http_request` went to, or was refused for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The field a `store` set, or was refused for: its name, never its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub outcome: AuditOutcome,
    /// Why it was denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whom a Gateway said the call was for: its claim, unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_for: Option<ClaimedFor>,
}

impl AuditRecord {
    /// A record of `pairing` doing `action` at `time`, about no Entry in particular.
    pub(crate) fn of(
        time: i64,
        pairing: &AgentPairing,
        action: AuditAction,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            time,
            pairing: pairing.name().to_owned(),
            pairing_id: pairing.id().as_str().to_owned(),
            kind: pairing.kind(),
            action,
            entry: None,
            program: None,
            method: None,
            host: None,
            field: None,
            outcome,
            reason: None,
            claimed_for: None,
        }
    }
}

/// Where an [`AgentSession`](crate::AgentSession) writes its records.
pub trait AuditSink: Send + Sync {
    fn record(&self, record: AuditRecord);
}

/// An append-only log file of JSON lines, rotated by size: when it would grow past its limit,
/// it is renamed to `<name>.1` (the older `.1` to `.2`, and so on), and the oldest is dropped.
pub struct AuditLog {
    path: PathBuf,
    max_bytes: u64,
    keep: u32,
    file: Mutex<Option<File>>,
}

impl AuditLog {
    /// The default size a log file grows to before it is rotated.
    pub const MAX_BYTES: u64 = 4 << 20;
    /// The default number of rotated files kept besides the current one.
    pub const KEEP: u32 = 4;

    /// The log at `path`, created with its directory when first written to.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_limits(path, Self::MAX_BYTES, Self::KEEP)
    }

    pub fn with_limits(path: impl Into<PathBuf>, max_bytes: u64, keep: u32) -> Self {
        Self {
            path: path.into(),
            max_bytes,
            keep,
            file: Mutex::new(None),
        }
    }

    /// The size a log file grows to before it is rotated.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// How many rotated files are kept besides the current one.
    pub fn keep(&self) -> u32 {
        self.keep
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn rotated(&self, n: u32) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    fn append(&self, line: &[u8]) -> io::Result<()> {
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let size = match file.as_ref() {
            Some(open) => open.metadata()?.len(),
            None => fs::metadata(&self.path).map(|meta| meta.len()).unwrap_or(0),
        };
        if size > 0 && size + line.len() as u64 > self.max_bytes {
            *file = None;
            if self.keep == 0 {
                fs::remove_file(&self.path)?;
            } else {
                for n in (1..self.keep).rev() {
                    let from = self.rotated(n);
                    if from.exists() {
                        fs::rename(&from, self.rotated(n + 1))?;
                    }
                }
                fs::rename(&self.path, self.rotated(1))?;
            }
        }
        if file.is_none() {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            // The log names Entries and pairings: only the host's own user reads it.
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            *file = Some(options.open(&self.path)?);
        }
        let open = file.as_mut().expect("opened above");
        open.write_all(line)?;
        open.flush()
    }

    /// The latest `limit` records, oldest first, from the rotated files too.
    pub fn read(&self, limit: usize) -> io::Result<Vec<AuditRecord>> {
        let mut records = Vec::new();
        let mut paths: Vec<PathBuf> = (1..=self.keep).rev().map(|n| self.rotated(n)).collect();
        paths.push(self.path.clone());
        for path in paths {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for line in BufReader::new(file).lines() {
                // A line cut short by a crash, or one a later keefast can't share, is skipped.
                if let Ok(record) = serde_json::from_str(&line?) {
                    records.push(record);
                }
            }
        }
        let skip = records.len().saturating_sub(limit);
        Ok(records.split_off(skip))
    }
}

/// A log file size as an operator writes it, for `serve --log-max-size`: bytes, or a number
/// with a `K`, `M` or `G` suffix (`KiB`, `MiB`, `GiB` too), in powers of 1024, such as `16M`.
pub fn parse_log_size(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    let digits = trimmed.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = &trimmed[digits.len()..];
    let shift = match unit.to_ascii_uppercase().as_str() {
        "" | "B" => 0,
        "K" | "KB" | "KIB" => 10,
        "M" | "MB" | "MIB" => 20,
        "G" | "GB" | "GIB" => 30,
        _ => return Err(format!("{text}: give a size such as 4M, 512K or 1G")),
    };
    let number: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("{text}: give a size such as 4M, 512K or 1G"))?;
    let bytes = number
        .checked_mul(1 << shift)
        .ok_or_else(|| format!("{text}: that is too large"))?;
    // Smaller than a record or two, every write would rotate.
    if bytes < 4096 {
        return Err(format!("{text}: give at least 4K"));
    }
    Ok(bytes)
}

impl AuditSink for AuditLog {
    fn record(&self, record: AuditRecord) {
        let mut line = serde_json::to_vec(&record).expect("a record is plain JSON");
        line.push(b'\n');
        // An Agent's use isn't refused because the log can't be written; the operator is told.
        if let Err(error) = self.append(&line) {
            eprintln!(
                "keefast-agent: can't write the audit log {}: {error}",
                self.path.display()
            );
        }
    }
}
