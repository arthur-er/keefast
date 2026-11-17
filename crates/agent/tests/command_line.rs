//! `keefast-agent serve`, `unlock`, `lock` and `status` as an operator runs them, as processes:
//! serve starts locked, unlock reads the Master key from its input and opens the Vault, a wrong
//! Master key says so, and lock locks it again.

#![cfg(unix)]

mod support;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use support::{KEEPASSXC_KDBX4, PASSWORD, VaultFile};
use tempfile::TempDir;

const AGENT: &str = env!("CARGO_BIN_EXE_keefast-agent");

/// `keefast-agent serve`, stopped when dropped.
struct Serve {
    child: Child,
    socket: PathBuf,
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
        .args(["serve", "--vault"])
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
    Serve {
        child,
        socket,
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
        .args(["serve", "--listen", "[::1]:7807", "--vault"])
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
