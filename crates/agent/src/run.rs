//! `keefast-agent run`: runs a program with secrets in its environment, for an Agent with a
//! shell (ADR 0012 "How a secret is used").
//!
//! It asks keefast for the values the Secret references name, starts the program with them in
//! its environment, and passes its exit code through. The values never go through MCP or the
//! model: the Agent sees only the program's output, from which they are redacted. Redaction is
//! best effort (ADR 0012): it catches a value written whole, even split across writes, but not
//! one the program encodes, splits up or writes to a file.
//!
//! It reaches keefast at `KEEFAST_AGENT_URL` with the Agent pairing's token from
//! `KEEFAST_AGENT_TOKEN`, through the endpoint [`USE_SECRETS_PATH`] the Headless host serves
//! beside `/mcp`. It works the same on Linux and Windows.

use std::io::{self, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::{SecretValue, UseSecrets};

/// Where the Headless host answers `run`, beside its MCP endpoint at `/mcp`.
pub const USE_SECRETS_PATH: &str = "/agent/use-secrets";
/// What a value is replaced with in the program's output.
pub const REDACTED: &str = "[redacted by keefast]";
/// The exit code when `run` itself fails, before or instead of the program: as `env` and
/// `docker run` do, so an Agent can tell keefast's failures from the program's.
pub const FAILED: u8 = 125;

/// What `run` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// Each `--env NAME=<Secret reference>`.
    pub env: Vec<(String, String)>,
    /// The program, then its arguments: everything after `--`.
    pub command: Vec<String>,
}

impl RunArgs {
    /// Reads `--env NAME=<Secret reference>`… `--` program args….
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut env = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--" => {
                    let command: Vec<String> = iter.cloned().collect();
                    if command.is_empty() {
                        return Err("run needs a program to run after --".to_owned());
                    }
                    if env.is_empty() {
                        return Err(
                            "run needs at least one --env NAME=<Secret reference>".to_owned()
                        );
                    }
                    return Ok(Self { env, command });
                }
                "--env" | "-e" => {
                    let pair = iter.next().ok_or("--env needs NAME=<Secret reference>")?;
                    let (name, reference) = pair
                        .split_once('=')
                        .filter(|(name, reference)| !name.is_empty() && !reference.is_empty())
                        .ok_or_else(|| {
                            format!("--env {pair}: write it as NAME=<Secret reference>")
                        })?;
                    env.push((name.to_owned(), reference.to_owned()));
                }
                other => {
                    return Err(format!(
                        "unknown option {other}: put the program to run after --"
                    ));
                }
            }
        }
        Err("run needs -- and then the program to run".to_owned())
    }
}

/// Where keefast is, and the token to show it.
pub struct Connection {
    pub url: String,
    pub token: Zeroizing<String>,
}

impl Connection {
    /// From `KEEFAST_AGENT_URL` and `KEEFAST_AGENT_TOKEN`.
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("KEEFAST_AGENT_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .ok_or(
                "KEEFAST_AGENT_URL isn't set: set it to the keefast Headless host's address, such \
                 as https://keefast.example.net/mcp, and KEEFAST_AGENT_TOKEN to the Agent \
                 pairing's token",
            )?;
        let token = std::env::var("KEEFAST_AGENT_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
            .ok_or("KEEFAST_AGENT_TOKEN isn't set: set it to the Agent pairing's token")?;
        Ok(Self {
            url,
            token: Zeroizing::new(token),
        })
    }

    /// The endpoint at `path` beside the MCP endpoint: the URL given may be the host's own or
    /// its MCP endpoint's.
    fn endpoint(&self, path: &str) -> String {
        let base = self.url.trim_end_matches('/');
        let base = base.strip_suffix("/mcp").unwrap_or(base);
        format!("{base}{path}")
    }

    /// The values for `request`, in the order of its references.
    pub fn use_secrets(&self, request: &UseSecrets) -> Result<Vec<SecretValue>, String> {
        #[derive(Deserialize)]
        struct Values {
            values: Vec<SecretValue>,
        }
        let body = Zeroizing::new(serde_json::to_string(request).expect("a request is plain JSON"));
        let values: Values = self.post(USE_SECRETS_PATH, &body)?;
        Ok(values.values)
    }

    /// POSTs `body`, JSON, to the endpoint at `path`, and reads its JSON answer, or the message
    /// of keefast's refusal.
    pub(crate) fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &str,
    ) -> Result<T, String> {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_global(Some(Duration::from_secs(90)))
                .build(),
        );
        let endpoint = self.endpoint(path);
        let mut response = agent
            .post(&endpoint)
            .header("Authorization", &format!("Bearer {}", self.token.as_str()))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|error| format!("can't reach keefast at {endpoint}: {error}"))?;
        let status = response.status();
        let body = Zeroizing::new(
            response
                .body_mut()
                .with_config()
                .limit(16 << 20)
                .read_to_string()
                .map_err(|error| format!("keefast's answer couldn't be read: {error}"))?,
        );
        if status.is_success() {
            return serde_json::from_str(&body)
                .map_err(|error| format!("keefast's answer couldn't be read: {error}"));
        }
        #[derive(Deserialize)]
        struct Refusal {
            message: String,
        }
        Err(match serde_json::from_str::<Refusal>(&body) {
            Ok(refusal) => refusal.message,
            Err(_) if status.as_u16() == 401 => {
                "keefast refused the token in KEEFAST_AGENT_TOKEN: the Agent pairing may have \
                 been revoked"
                    .to_owned()
            }
            Err(_) => format!("keefast answered {status} at {endpoint}"),
        })
    }
}

/// Runs `args`: fetches its values through `connection`, then runs the program with them and
/// its output redacted. Returns the program's exit code.
pub fn run(args: &RunArgs, connection: &Connection) -> Result<u8, String> {
    let request = UseSecrets {
        references: args
            .env
            .iter()
            .map(|(_, reference)| reference.clone())
            .collect(),
        command: args.command.clone(),
    };
    let values = connection.use_secrets(&request)?;
    if values.len() != args.env.len() {
        return Err("keefast's answer didn't have a value for every --env".to_owned());
    }
    let mut command = Command::new(&args.command[0]);
    command
        .args(&args.command[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for ((name, _), value) in args.env.iter().zip(&values) {
        command.env(name, value.value());
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("can't start {}: {error}", args.command[0]))?;
    let secrets: Vec<Zeroizing<Vec<u8>>> = values
        .iter()
        .map(|value| Zeroizing::new(value.value().as_bytes().to_vec()))
        .collect();
    drop(values);
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let out = {
        let redactor = Redactor::new(secrets.clone());
        thread::spawn(move || copy_redacted(stdout, io::stdout(), redactor))
    };
    let err = {
        let redactor = Redactor::new(secrets);
        thread::spawn(move || copy_redacted(stderr, io::stderr(), redactor))
    };
    let status = child
        .wait()
        .map_err(|error| format!("lost track of {}: {error}", args.command[0]))?;
    // The output is copied to the end, even after the program exits.
    let _ = out.join();
    let _ = err.join();
    Ok(exit_code(status))
}

/// The exit code to pass on: the program's, or 128 plus the signal that killed it, as shells
/// report it.
fn exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        // Windows exit codes are 32-bit; the low byte is what a shell sees of a large one.
        return (code & 0xff) as u8;
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128u8.wrapping_add(signal as u8);
    }
    FAILED
}

fn copy_redacted(mut from: impl Read, mut to: impl Write, mut redactor: Redactor) {
    let mut buffer = [0u8; 8192];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let out = redactor.push(&buffer[..read]);
        // Whoever reads the output may go away; the program still runs to its end.
        if to.write_all(&out).and_then(|()| to.flush()).is_err() {
            continue;
        }
    }
    let _ = to.write_all(&redactor.finish());
    let _ = to.flush();
}

/// A streaming filter that replaces secret values with [`REDACTED`]. It holds back only the
/// end of the output that could be the start of a value, until the next chunk says whether it
/// is, so a value split across chunks is still caught.
pub struct Redactor {
    /// Longest first, so of two values starting at the same place, the longer is replaced.
    values: Vec<Zeroizing<Vec<u8>>>,
    pending: Zeroizing<Vec<u8>>,
}

impl Redactor {
    pub fn new(mut values: Vec<Zeroizing<Vec<u8>>>) -> Self {
        values.retain(|value| !value.is_empty());
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Self {
            values,
            pending: Zeroizing::new(Vec::new()),
        }
    }

    /// The output that can be passed on after `chunk`, redacted.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        self.drain(false)
    }

    /// The rest of the output, at its end.
    pub fn finish(&mut self) -> Vec<u8> {
        self.drain(true)
    }

    fn drain(&mut self, end: bool) -> Vec<u8> {
        let buffer = std::mem::replace(&mut self.pending, Zeroizing::new(Vec::new()));
        let mut out = Vec::with_capacity(buffer.len());
        let mut at = 0;
        while at < buffer.len() {
            let rest = &buffer[at..];
            if let Some(value) = self.values.iter().find(|value| rest.starts_with(value)) {
                out.extend_from_slice(REDACTED.as_bytes());
                at += value.len();
                continue;
            }
            if !end
                && self
                    .values
                    .iter()
                    .any(|value| value.len() > rest.len() && value.starts_with(rest))
            {
                self.pending.extend_from_slice(rest);
                break;
            }
            out.push(buffer[at]);
            at += 1;
        }
        out
    }
}
