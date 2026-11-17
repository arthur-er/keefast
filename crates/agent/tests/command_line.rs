//! `keefast-agent serve`, `unlock`, `lock` and `status` as an operator runs them, as processes:
//! serve starts locked, unlock reads the Master key from its input and opens the Vault, a wrong
//! Master key says so, and lock locks it again; `pairing` pairs an Agent and prints its token
//! once, lists the pairings and revokes one; and `run` uses a secret once the desktop app
//! allows it, is denied before, and `log` shows both (#156's "Done when", on one machine).

#![cfg(unix)]

mod support;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use keefast_agent::approver::ApproverAnswer;
use keefast_agent::approver_client::{
    AgentHost, ApproverClient, ApproverClientEvent, answer_prompt,
};
use keefast_agent::{AgentDecision, AgentPrompt, AgentSettingsExt, AgentUse};
use keefast_core::EntryFields;
use secrecy::SecretString;
use support::{FakeClock, KEEPASSXC_KDBX4, PASSWORD, VaultFile};
use tempfile::TempDir;

const AGENT: &str = env!("CARGO_BIN_EXE_keefast-agent");

/// `keefast-agent serve`, stopped when dropped.
struct Serve {
    child: Child,
    socket: PathBuf,
    /// Where Agents reach it, from the line it prints when it starts.
    url: std::sync::mpsc::Receiver<String>,
    _dir: TempDir,
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve(file: &VaultFile) -> Serve {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    let child = Command::new(AGENT)
        // Port 0, as tests run side by side: the default port would be taken.
        .args(["serve", "--listen", "127.0.0.1:0", "--vault"])
        .arg(&file.path)
        .arg("--admin-socket")
        .arg(&socket)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = child;
    let stderr = child.stderr.take().unwrap();
    let (send, url) = std::sync::mpsc::channel();
    thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if let Some((_, url)) = line.split_once("Agents connect to ") {
                let _ = send.send(url.trim().to_owned());
            }
        }
    });
    let until = Instant::now() + Duration::from_secs(10);
    while !socket.exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    Serve {
        child,
        socket,
        url,
        _dir: dir,
    }
}

fn run(socket: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(AGENT)
        .args(args)
        .arg("--admin-socket")
        .arg(socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr)
}

#[test]
fn serve_starts_locked_and_unlock_opens_it_with_the_master_key() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let before = run(&served.socket, &["status"], "");
    let unlock = run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    let after = run(&served.socket, &["status"], "");

    assert!(before.status.success(), "{}", text(&before));
    assert!(text(&before).contains("locked"), "{}", text(&before));
    assert!(unlock.status.success(), "{}", text(&unlock));
    assert!(text(&after).contains("unlocked"), "{}", text(&after));
}

#[test]
fn unlock_with_a_wrong_master_key_says_so_and_fails() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let unlock = run(&served.socket, &["unlock"], "not-the-password\n");

    assert!(!unlock.status.success());
    assert!(
        text(&unlock).contains("Master key is wrong"),
        "{}",
        text(&unlock)
    );
}

#[test]
fn lock_locks_the_vault_again() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));

    let lock = run(&served.socket, &["lock"], "");
    let status = run(&served.socket, &["status"], "");

    assert!(lock.status.success(), "{}", text(&lock));
    assert!(!text(&status).contains("unlocked"), "{}", text(&status));
}

/// `keefast-agent serve` with these arguments, run to see whether it refuses to start.
fn serve_with(file: &VaultFile, args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    Command::new(AGENT)
        .args(["serve", "--vault"])
        .arg(&file.path)
        .arg("--admin-socket")
        .arg(dir.path().join("admin.sock"))
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn serve_refuses_a_listen_address_other_machines_reach_without_tls() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);

    for address in ["0.0.0.0:7807", "192.0.2.10:7807", "[::]:7807"] {
        let refused = serve_with(&file, &["--listen", address]);

        assert!(!refused.status.success(), "{address}: {}", text(&refused));
        assert!(
            text(&refused).contains("TLS"),
            "{address}: {}",
            text(&refused)
        );
    }
}

#[test]
fn serve_refuses_half_of_a_tls_setup() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);

    let refused = serve_with(
        &file,
        &[
            "--listen",
            "0.0.0.0:7807",
            "--tls-cert",
            "/etc/ssl/agents.pem",
        ],
    );

    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("--tls-key"), "{}", text(&refused));
}

#[test]
fn serve_refuses_a_listen_address_it_cannot_read() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);

    let refused = serve_with(&file, &["--listen", "agents.example.com"]);

    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("--listen"), "{}", text(&refused));
}

#[test]
fn serve_listens_on_loopback_without_tls() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("admin.sock");
    let mut child = Command::new(AGENT)
        .args(["serve", "--listen", "[::1]:0", "--vault"])
        .arg(&file.path)
        .arg("--admin-socket")
        .arg(&socket)
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !socket.exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }

    let status = run(&socket, &["status"], "");
    let _ = child.kill();
    let _ = child.wait();

    assert!(status.status.success(), "{}", text(&status));
}

#[test]
fn pairing_create_prints_the_token_once_and_list_shows_the_pairing() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));

    let created = run(
        &served.socket,
        &["pairing", "create", "claude", "--agent-group", "Agents"],
        "",
    );
    let listed = run(&served.socket, &["pairing", "list"], "");

    assert!(created.status.success(), "{}", text(&created));
    let token = String::from_utf8_lossy(&created.stdout).trim().to_owned();
    assert!(token.starts_with("kfa_"), "{}", text(&created));
    assert!(listed.status.success(), "{}", text(&listed));
    assert!(
        text(&listed).contains("Agent group: Agents"),
        "{}",
        text(&listed)
    );
    assert!(text(&listed).contains("claude"), "{}", text(&listed));
    assert!(!text(&listed).contains(&token), "{}", text(&listed));
}

#[test]
fn pairing_create_without_an_agent_group_says_how_to_choose_one() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));

    let refused = run(&served.socket, &["pairing", "create", "claude"], "");

    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("--agent-group"),
        "{}",
        text(&refused)
    );
}

#[test]
fn pairing_needs_the_vault_unlocked() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);

    let refused = run(&served.socket, &["pairing", "list"], "");

    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(text(&refused).contains("locked"), "{}", text(&refused));
}

#[test]
fn pairing_revoke_removes_the_pairing_and_keeps_it_removed_after_a_restart() {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    run(
        &served.socket,
        &["pairing", "create", "claude", "--agent-group", "Agents"],
        "",
    );
    run(
        &served.socket,
        &["pairing", "create", "codex", "--reads", "claude"],
        "",
    );

    let revoked = run(&served.socket, &["pairing", "revoke", "claude"], "");
    drop(served);
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    let listed = run(&served.socket, &["pairing", "list"], "");

    assert!(revoked.status.success(), "{}", text(&revoked));
    assert!(
        text(&listed).contains("codex, which also reads claude"),
        "{}",
        text(&listed)
    );
    assert!(!text(&listed).contains("\nclaude"), "{}", text(&listed));
}

/// The status of opening the Approver prompt stream at the host `url` with `token`.
fn approver_stream_status(url: &str, token: &str) -> u16 {
    let events = url.replace("/mcp", "/approver/events");
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_recv_body(Some(Duration::from_millis(100)))
        .build()
        .new_agent();
    agent
        .get(&events)
        .header("authorization", &format!("Bearer {token}"))
        .call()
        .unwrap()
        .status()
        .as_u16()
}

#[test]
fn pairing_approver_prints_an_approvers_token_once_even_while_locked_and_revoking_it_shuts_it_out()
{
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let served = serve(&file);
    let url = served.url.recv_timeout(Duration::from_secs(10)).unwrap();

    let paired = run(&served.socket, &["pairing", "approver", "laptop"], "");
    let token = String::from_utf8_lossy(&paired.stdout).trim().to_owned();
    let listed = run(&served.socket, &["pairing", "approvers"], "");
    let opened = approver_stream_status(&url, &token);
    let revoked = run(
        &served.socket,
        &["pairing", "revoke-approver", "laptop"],
        "",
    );
    let shut_out = approver_stream_status(&url, &token);
    let again = run(
        &served.socket,
        &["pairing", "revoke-approver", "laptop"],
        "",
    );

    assert!(paired.status.success(), "{}", text(&paired));
    assert!(token.starts_with("kfapprover_"), "{}", text(&paired));
    assert!(
        text(&paired).contains("Settings → Agent hosts"),
        "{}",
        text(&paired)
    );
    assert!(text(&listed).contains("laptop"), "{}", text(&listed));
    assert!(!text(&listed).contains(&token), "{}", text(&listed));
    assert_eq!(opened, 200);
    assert!(revoked.status.success(), "{}", text(&revoked));
    assert_eq!(shut_out, 401);
    assert!(!again.status.success(), "{}", text(&again));
}

/// A Vault file with an Agent group, `Agents`, holding `GitHub/token`, which the pairing
/// `claude` reads; and claude's token.
fn vault_with_a_github_token() -> (VaultFile, String) {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let agents = workspace.create_group(vault, root, "Agents").unwrap();
    let github = workspace.create_group(vault, agents, "GitHub").unwrap();
    workspace.set_agent_group(vault, agents).unwrap();
    let claude = workspace
        .create_agent_pairing(vault, "claude", vec![github], false)
        .unwrap();
    let fields = EntryFields {
        title: "token".to_owned(),
        password: SecretString::from("ghp_the-real-token-value"),
        ..EntryFields::default()
    };
    workspace.create_entry(vault, github, &fields).unwrap();
    workspace.save_vault(vault).unwrap();
    (file, claude.token.to_string())
}

/// The desktop app allowing claude to use `GitHub/token` with `program`, in the Entry
/// editor: a decision saved into the Vault file, which the Headless host Merges.
fn allow_in_the_desktop_app(file: &VaultFile, program: &str) {
    let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_100));
    let settings = workspace.agent_settings(vault).unwrap();
    let claude = settings.pairings()[0].id().clone();
    let entry = workspace
        .readable_vault(vault)
        .unwrap()
        .vault()
        .root()
        .all_entries()
        .find(|entry| entry.title() == "token")
        .unwrap()
        .id();
    workspace
        .remember_agent_decision(
            vault,
            entry,
            &claude,
            &AgentUse::Program(program.to_owned()),
            AgentDecision::Allow,
        )
        .unwrap();
    workspace.save_vault(vault).unwrap();
}

fn run_with_token(url: &str, token: &str) -> Output {
    Command::new(AGENT)
        .args(["run", "--env", "GH_TOKEN=GitHub/token#password", "--"])
        .args(["printenv", "GH_TOKEN"])
        .env("KEEFAST_AGENT_URL", url)
        .env("KEEFAST_AGENT_TOKEN", token)
        .output()
        .unwrap()
}

#[test]
fn run_is_denied_until_the_desktop_app_allows_it_and_the_log_shows_both() {
    let (file, token) = vault_with_a_github_token();
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    let url = served.url.recv_timeout(Duration::from_secs(10)).unwrap();

    let denied = run_with_token(&url, &token);
    allow_in_the_desktop_app(&file, "printenv");
    let until = Instant::now() + Duration::from_secs(15);
    let allowed = loop {
        let ran = run_with_token(&url, &token);
        if ran.status.success() || Instant::now() > until {
            break ran;
        }
        thread::sleep(Duration::from_millis(200));
    };
    let log = run(&served.socket, &["log"], "");

    assert_eq!(denied.status.code(), Some(125), "{}", text(&denied));
    assert!(text(&denied).contains("Entry editor"), "{}", text(&denied));
    assert!(allowed.status.success(), "{}", text(&allowed));
    // printenv printed the value, and run redacted it.
    assert_eq!(
        String::from_utf8_lossy(&allowed.stdout).trim(),
        "[redacted by keefast]"
    );
    let log = text(&log);
    assert!(log.contains("claude  use token"), "{log}");
    assert!(log.contains("with printenv  denied"), "{log}");
    assert!(log.contains("with printenv  allowed"), "{log}");
    assert!(!log.contains("ghp_the-real-token-value"), "{log}");
}

/// A desktop keefast paired with the host as an Approver, connected, answering every prompt it
/// hears with `answer`; and the prompts it heard.
fn approver_answering(
    socket: &Path,
    url: &str,
    answer: ApproverAnswer,
) -> (ApproverClient, std::sync::mpsc::Receiver<AgentPrompt>) {
    let paired = run(socket, &["pairing", "approver", "desktop"], "");
    let token = String::from_utf8_lossy(&paired.stdout).trim().to_owned();
    let host = AgentHost {
        url: url.to_owned(),
        token: zeroize::Zeroizing::new(token),
    };
    let (heard, prompts) = std::sync::mpsc::channel();
    let (connected, is_connected) = std::sync::mpsc::channel();
    let answering = host.clone();
    let client = ApproverClient::start(host, move |event| match event {
        ApproverClientEvent::Connected => {
            let _ = connected.send(());
        }
        ApproverClientEvent::Prompt { id, prompt, .. } => {
            let _ = heard.send(prompt);
            // Answered from another thread, as the desktop's user would, while the stream reads on.
            let answering = answering.clone();
            thread::spawn(move || answer_prompt(&answering, &id, answer));
        }
        _ => {}
    });
    is_connected.recv_timeout(Duration::from_secs(10)).unwrap();
    (client, prompts)
}

#[test]
fn run_against_a_remote_host_prompts_the_approver_and_a_remembered_allow_skips_the_prompt() {
    let (file, token) = vault_with_a_github_token();
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    let url = served.url.recv_timeout(Duration::from_secs(10)).unwrap();
    let allow = ApproverAnswer {
        allow: true,
        remember: true,
    };
    let (_desktop, prompts) = approver_answering(&served.socket, &url, allow);

    let first = run_with_token(&url, &token);
    let second = run_with_token(&url, &token);

    assert!(first.status.success(), "{}", text(&first));
    assert!(second.status.success(), "{}", text(&second));
    let heard: Vec<AgentPrompt> = prompts.try_iter().collect();
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].command, ["printenv", "GH_TOKEN"]);
    let log = text(&run(&served.socket, &["log"], ""));
    assert!(
        log.contains("with printenv  allowed: allowed and remembered at a prompt"),
        "{log}"
    );
}

#[test]
fn run_is_denied_when_the_approver_denies() {
    let (file, token) = vault_with_a_github_token();
    let served = serve(&file);
    run(&served.socket, &["unlock"], &format!("{PASSWORD}\n"));
    let url = served.url.recv_timeout(Duration::from_secs(10)).unwrap();
    let deny = ApproverAnswer {
        allow: false,
        remember: false,
    };
    let (_desktop, prompts) = approver_answering(&served.socket, &url, deny);

    let denied = run_with_token(&url, &token);

    assert_eq!(denied.status.code(), Some(125), "{}", text(&denied));
    assert!(text(&denied).contains("doesn't allow"), "{}", text(&denied));
    assert_eq!(prompts.try_iter().count(), 1);
}
