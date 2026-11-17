//! `keefast-agent run` as a process, against a fake keefast host (spec Seam 3): the values
//! reach the program's environment, are redacted from its output even when split across
//! writes, and its exit code is passed through. It runs on Linux and Windows.
//!
//! The program `run` starts is this test binary again, running [`child`], which does what
//! the `KEEFAST_RUN_CHILD` variable says: no shell is needed, so it works on both.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use keefast_agent::UseSecrets;

const CHILD: &str = "KEEFAST_RUN_CHILD";
const TOKEN: &str = "kfa_the-pairings-token";
const VALUE: &str = "ghp_s3cr3t-value-0123456789";

/// The program `run` starts in these tests; does nothing in a normal test run.
#[test]
fn child() {
    let Ok(what) = std::env::var(CHILD) else {
        return;
    };
    let token = std::env::var("GH_TOKEN").unwrap_or_default();
    let mut stdout = std::io::stdout();
    match what.as_str() {
        "print" => {
            let reversed: String = token.chars().rev().collect();
            println!("GH_TOKEN={token}");
            println!("reversed={reversed}");
            eprintln!("on stderr too: {token}");
        }
        "split" => {
            // Each half its own write, with time between, so `run` reads them apart.
            let (first, second) = token.split_at(token.len() / 2);
            write!(stdout, "token: {first}").unwrap();
            stdout.flush().unwrap();
            thread::sleep(Duration::from_millis(300));
            writeln!(stdout, "{second} done").unwrap();
            stdout.flush().unwrap();
        }
        "fail" => {
            println!("failing on purpose");
            stdout.flush().unwrap();
            std::process::exit(7);
        }
        other => panic!("unknown child {other}"),
    }
    stdout.flush().unwrap();
    std::process::exit(0);
}

/// A keefast host that knows one Secret reference, `GitHub/token#password`, and the requests
/// it was sent.
struct FakeHost {
    url: String,
    requests: Arc<Mutex<Vec<UseSecrets>>>,
}

impl FakeHost {
    /// Answers with `refusal` (status and message) if given, else with the values.
    fn start(refusal: Option<(u16, &'static str)>) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/mcp", server.server_addr().to_ip().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let values = HashMap::from([("GitHub/token#password", VALUE)]);
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let authorized = request.headers().iter().any(|header| {
                    header.field.equiv("Authorization")
                        && header.value.as_str() == format!("Bearer {TOKEN}")
                });
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let (status, reply) = if request.url() != "/agent/use-secrets" {
                    (404, String::new())
                } else if !authorized {
                    (401, String::new())
                } else if let Some((status, message)) = refusal {
                    (
                        status,
                        serde_json::json!({ "error": "denied", "message": message }).to_string(),
                    )
                } else {
                    let asked: UseSecrets = serde_json::from_str(&body).unwrap();
                    let found: Vec<_> = asked
                        .references
                        .iter()
                        .map(|reference| {
                            serde_json::json!({
                                "reference": reference,
                                "value": values[reference.as_str()],
                            })
                        })
                        .collect();
                    seen.lock().unwrap().push(asked);
                    (200, serde_json::json!({ "values": found }).to_string())
                };
                let _ = request
                    .respond(tiny_http::Response::from_string(reply).with_status_code(status));
            }
        });
        Self { url, requests }
    }

    /// Runs `keefast-agent run --env GH_TOKEN=GitHub/token#password -- <this test, as child>`.
    fn run(&self, child: &str) -> Output {
        let me = std::env::current_exe().unwrap();
        Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
            .args(["run", "--env", "GH_TOKEN=GitHub/token#password", "--"])
            .arg(&me)
            .args(["--exact", "child", "--nocapture", "--test-threads=1"])
            .env(CHILD, child)
            .env("KEEFAST_AGENT_URL", &self.url)
            .env("KEEFAST_AGENT_TOKEN", TOKEN)
            .output()
            .unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_value_reaches_the_programs_environment_but_not_its_output() {
    let host = FakeHost::start(None);

    let output = host.run("print");

    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let reversed: String = VALUE.chars().rev().collect();
    assert!(stdout.contains(&format!("reversed={reversed}")), "{stdout}");
    assert!(
        stdout.contains("GH_TOKEN=[redacted by keefast]"),
        "{stdout}"
    );
    assert!(
        stderr.contains("on stderr too: [redacted by keefast]"),
        "{stderr}"
    );
    assert!(!stdout.contains(VALUE) && !stderr.contains(VALUE));
}

#[test]
fn a_value_split_across_writes_is_still_redacted() {
    let host = FakeHost::start(None);

    let output = host.run("split");

    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("token: [redacted by keefast] done"),
        "{stdout}"
    );
    assert!(!stdout.contains(&VALUE[..VALUE.len() / 2]), "{stdout}");
}

#[test]
fn the_programs_exit_code_is_passed_through() {
    let host = FakeHost::start(None);

    let output = host.run("fail");

    assert_eq!(output.status.code(), Some(7), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("failing on purpose"));
}

#[test]
fn keefast_is_told_the_whole_command_line_and_the_references() {
    let host = FakeHost::start(None);

    host.run("print");

    let requests = host.requests.lock().unwrap();
    let [request] = requests.as_slice() else {
        panic!("{requests:?}");
    };
    assert_eq!(request.references, ["GitHub/token#password"]);
    let me = std::env::current_exe().unwrap();
    assert_eq!(request.command[0], me.to_string_lossy());
    assert_eq!(
        request.command[1..],
        ["--exact", "child", "--nocapture", "--test-threads=1"]
    );
}

#[test]
fn a_refusal_is_shown_and_the_program_never_starts() {
    let host = FakeHost::start(Some((
        403,
        "the user hasn't allowed this Agent pairing to use GitHub/token#password with gh",
    )));

    let output = host.run("print");

    assert_eq!(output.status.code(), Some(125));
    assert!(
        text(&output.stderr).contains("hasn't allowed this Agent pairing"),
        "{}",
        text(&output.stderr)
    );
    assert!(!text(&output.stdout).contains("reversed="));
}

#[test]
fn without_a_token_run_says_what_to_set() {
    let output = Command::new(env!("CARGO_BIN_EXE_keefast-agent"))
        .args(["run", "--env", "GH_TOKEN=GitHub/token#password", "--", "gh"])
        .env_remove("KEEFAST_AGENT_URL")
        .env_remove("KEEFAST_AGENT_TOKEN")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(125));
    assert!(text(&output.stderr).contains("KEEFAST_AGENT_TOKEN"));
}
