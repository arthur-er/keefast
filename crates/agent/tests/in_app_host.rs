//! The in-app host's link (ADR 0012 update "In-app host"): `keefast-agent mcp`, `run` and
//! `store` reach the desktop app over its own socket or pipe, as the native host reaches it for
//! the browser (spec: "the in-app relay reuses the native host's socket-pair test approach").
//!
//! A fake app answers at a private endpoint, playing the desktop app's part at the process
//! boundary; the desktop app's own handling is tested in the app crate. A closed app answers
//! as a locked one does. It runs on Linux and Windows.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use keefast_agent::local::{self, DesktopApp};
use keefast_agent::{
    AgentError, AgentReply, AgentRequest, SecretListing, SecretReference, SecretValue,
    ServesAgents, WrittenEntry,
};
use keefast_native_host::ipc;
use zeroize::Zeroizing;

const TOKEN: &str = "kfa_the-pairings-token";
const VALUE: &str = "ghp_s3cr3t-value-0123456789";
const CHILD: &str = "KEEFAST_IN_APP_CHILD";

/// The program `run` starts in these tests; does nothing in a normal test run.
#[test]
fn child() {
    if std::env::var(CHILD).is_err() {
        return;
    }
    println!("GH_TOKEN={}", std::env::var("GH_TOKEN").unwrap_or_default());
    std::io::stdout().flush().unwrap();
    std::process::exit(0);
}

/// The desktop app's part: unlocked, with one Entry `claude/GitHub` for the pairing whose
/// token is [`TOKEN`], or locked. It keeps what it was asked.
struct FakeApp {
    locked: Mutex<bool>,
    asked: Mutex<Vec<(String, AgentRequest)>>,
}

impl ServesAgents for FakeApp {
    fn handle_agent(&self, token: &str, request: AgentRequest) -> Result<AgentReply, AgentError> {
        self.asked
            .lock()
            .unwrap()
            .push((token.to_owned(), request.clone()));
        if *self.locked.lock().unwrap() {
            return Err(AgentError::Locked);
        }
        if token != TOKEN {
            return Err(AgentError::NotPaired);
        }
        match request {
            AgentRequest::ListSecrets => Ok(AgentReply::Secrets(vec![listing()])),
            AgentRequest::UseSecrets(used) if !used.command.is_empty() => {
                Ok(AgentReply::Values(
                    used.references
                        .into_iter()
                        .map(|reference| SecretValue::new(reference, Zeroizing::new(VALUE.into())))
                        .collect(),
                ))
            }
            AgentRequest::Store(_) => Ok(AgentReply::Written(WrittenEntry {
                reference: listing().reference,
                created: true,
            })),
            AgentRequest::Reveal(reference) => Err(AgentError::Denied {
                reference,
                program: "reveal_secret".to_owned(),
            }),
            _ => Err(AgentError::NoProgram),
        }
    }
}

fn listing() -> SecretListing {
    SecretListing {
        title: "GitHub".to_owned(),
        username: "octocat".to_owned(),
        url: String::new(),
        tags: Vec::new(),
        fields: vec!["password".to_owned()],
        reference: SecretReference {
            uuid: "00112233445566778899aabbccddeeff".to_owned(),
            path: "claude/GitHub".to_owned(),
        },
        hint: None,
    }
}

/// A private endpoint for one test: a socket in a directory of its own, or a pipe named after
/// the test.
struct Endpoint {
    path: PathBuf,
    _dir: Option<tempfile::TempDir>,
}

fn endpoint(test: &str) -> Endpoint {
    if cfg!(windows) {
        Endpoint {
            path: PathBuf::from(format!(
                r"\\.\pipe\keefast-test-{}-{test}",
                std::process::id()
            )),
            _dir: None,
        }
    } else {
        let dir = tempfile::tempdir().unwrap();
        Endpoint {
            path: dir.path().join("keefast").join("agent.sock"),
            _dir: Some(dir),
        }
    }
}

/// The fake app, serving at its endpoint until dropped.
struct Running {
    app: Arc<FakeApp>,
    endpoint: Endpoint,
    _server: ipc::Server,
}

fn app(test: &str) -> Running {
    let app = Arc::new(FakeApp {
        locked: Mutex::new(false),
        asked: Mutex::default(),
    });
    let endpoint = endpoint(test);
    let server = local::serve(endpoint.path.clone(), app.clone()).unwrap();
    Running {
        app,
        endpoint,
        _server: server,
    }
}

#[test]
fn a_call_reaches_the_app_with_the_pairings_token_and_its_answer_comes_back() {
    let running = app("list");

    let reply = DesktopApp::at(running.endpoint.path.clone())
        .call(TOKEN, AgentRequest::ListSecrets)
        .unwrap();

    assert_eq!(reply.into_secrets(), vec![listing()]);
    assert_eq!(
        running.app.asked.lock().unwrap().clone(),
        vec![(TOKEN.to_owned(), AgentRequest::ListSecrets)]
    );
}

#[test]
fn a_refusal_comes_back_as_the_app_gave_it() {
    let running = app("refusal");
    let desktop = DesktopApp::at(running.endpoint.path.clone());

    assert_eq!(
        desktop.call(
            TOKEN,
            AgentRequest::Reveal("claude/GitHub#password".to_owned())
        ),
        Err(AgentError::Denied {
            reference: "claude/GitHub#password".to_owned(),
            program: "reveal_secret".to_owned(),
        })
    );
    assert_eq!(
        desktop.call("kfa_someone-elses", AgentRequest::ListSecrets),
        Err(AgentError::NotPaired)
    );
}

#[test]
fn a_closed_app_answers_as_a_locked_one() {
    let closed = endpoint("closed");

    assert_eq!(
        DesktopApp::at(closed.path.clone()).call(TOKEN, AgentRequest::ListSecrets),
        Err(AgentError::Locked)
    );
}

#[test]
fn a_keefast_agent_of_another_version_is_told_to_update_both() {
    let running = app("version");
    let mut stream = ipc::connect(&running.endpoint.path).unwrap();
    let call =
        serde_json::json!({ "version": 99, "id": 7, "token": TOKEN, "request": "list_secrets" });

    ipc::write_frame(&mut stream, call.to_string().as_bytes()).unwrap();
    let answer: serde_json::Value =
        serde_json::from_slice(&ipc::read_frame(&mut stream).unwrap().unwrap()).unwrap();

    assert_eq!(answer["id"], 7);
    let message = answer["answer"]["Err"]["mismatch"].as_str().unwrap();
    assert!(message.contains("version 99"), "{answer}");
    assert!(running.app.asked.lock().unwrap().is_empty());
}

/// `keefast-agent mcp`, started as an MCP client starts it, reaching the app at `endpoint`.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start(endpoint: &Endpoint) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
            .arg("mcp")
            .env(local::ENDPOINT_ENV, &endpoint.path)
            .env("KEEFAST_AGENT_TOKEN", TOKEN)
            .env_remove("KEEFAST_AGENT_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut mcp = Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            next: 0,
        };
        let initialized = mcp.ask(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            }),
        );
        assert!(
            initialized["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("list_secrets")
        );
        mcp.send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        mcp
    }

    fn send(&mut self, message: serde_json::Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn ask(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next += 1;
        let id = self.next;
        self.send(
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
        loop {
            let mut line = String::new();
            assert!(self.stdout.read_line(&mut line).unwrap() > 0, "mcp ended");
            let message: serde_json::Value = serde_json::from_str(&line).unwrap();
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Calls the tool `name`: whether it failed, and its text.
    fn call(&mut self, name: &str) -> (bool, String) {
        let answer = self.ask(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": {} }),
        );
        let result = &answer["result"];
        (
            result["isError"].as_bool().unwrap_or(false),
            result["content"][0]["text"].as_str().unwrap().to_owned(),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn keefast_agent_mcp_serves_the_same_tools_from_the_app() {
    let running = app("mcp-tools");
    let mut mcp = Mcp::start(&running.endpoint);

    let tools = mcp.ask("tools/list", serde_json::json!({}));
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    let (failed, listed) = mcp.call("list_secrets");

    assert_eq!(
        names,
        [
            "create_entry",
            "http_request",
            "list_secrets",
            "reveal_secret"
        ]
    );
    assert!(!failed, "{listed}");
    assert!(listed.contains("<untrusted-vault-content>"), "{listed}");
    assert!(listed.contains("claude/GitHub"), "{listed}");
}

#[test]
fn keefast_agent_mcp_says_locked_while_the_app_is_locked_or_closed_and_recovers() {
    let running = app("mcp-locked");
    let mut mcp = Mcp::start(&running.endpoint);
    *running.app.locked.lock().unwrap() = true;

    let (failed, said) = mcp.call("list_secrets");
    assert!(failed);
    assert!(said.contains("keefast is locked"), "{said}");

    *running.app.locked.lock().unwrap() = false;
    let (failed, said) = mcp.call("list_secrets");
    assert!(!failed, "{said}");

    // Quit the app: the MCP server stays up, and says locked.
    let Running {
        endpoint, _server, ..
    } = running;
    drop(_server);
    let (failed, said) = mcp.call("list_secrets");
    assert!(failed);
    assert!(said.contains("keefast is locked"), "{said}");
    drop(endpoint);
}

#[test]
fn run_without_a_hosts_address_gets_its_values_from_the_app() {
    let running = app("run");
    let me = std::env::current_exe().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
        .args(["run", "--env", "GH_TOKEN=claude/GitHub#password", "--"])
        .arg(&me)
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env(local::ENDPOINT_ENV, &running.endpoint.path)
        .env("KEEFAST_AGENT_TOKEN", TOKEN)
        .env_remove("KEEFAST_AGENT_URL")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("GH_TOKEN=[redacted by keefast]"),
        "{stdout}"
    );
    let asked = running.app.asked.lock().unwrap().clone();
    assert!(
        matches!(&asked[..], [(token, AgentRequest::UseSecrets(used))]
        if token == TOKEN && used.references == ["claude/GitHub#password"])
    );
}

#[test]
fn run_without_a_hosts_address_or_a_running_app_says_locked() {
    let closed = endpoint("run-closed");

    let output = Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
        .args([
            "run",
            "--env",
            "GH_TOKEN=claude/GitHub#password",
            "--",
            "gh",
        ])
        .env(local::ENDPOINT_ENV, &closed.path)
        .env("KEEFAST_AGENT_TOKEN", TOKEN)
        .env_remove("KEEFAST_AGENT_URL")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(125));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("keefast is locked"), "{stderr}");
}

#[test]
fn store_without_a_hosts_address_saves_through_the_app() {
    let running = app("store");

    let mut child = Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
        .args(["store", "claude/GitHub#password"])
        .env(local::ENDPOINT_ENV, &running.endpoint.path)
        .env("KEEFAST_AGENT_TOKEN", TOKEN)
        .env_remove("KEEFAST_AGENT_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{VALUE}").unwrap();
    let output = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("Saved to a new Entry claude/GitHub"),
        "{stdout}"
    );
    let asked = running.app.asked.lock().unwrap().clone();
    assert!(matches!(&asked[..], [(_, AgentRequest::Store(stored))]
        if stored.reference == "claude/GitHub#password" && stored.value() == VALUE));
}
