//! The Headless host's audit log file: append-only JSON lines, rotated by size so it never
//! fills the disk, and read back newest last across the rotated files (ADR 0012 "Audit").

use keefast_agent::{AuditAction, AuditEntry, AuditLog, AuditOutcome, AuditRecord, AuditSink};

fn record(n: i64) -> AuditRecord {
    AuditRecord {
        time: 1_900_000_000 + n,
        pairing: "claude".to_owned(),
        pairing_id: "0123".to_owned(),
        kind: keefast_agent::PairingKind::Agent,
        action: AuditAction::Use,
        entry: Some(AuditEntry {
            uuid: "00112233445566778899aabbccddeeff".to_owned(),
            title: "GitHub".to_owned(),
        }),
        method: None,
        host: None,
        field: None,
        program: Some("gh".to_owned()),
        outcome: AuditOutcome::Allowed,
        reason: None,
    }
}

#[test]
fn records_are_read_back_in_the_order_they_were_written() {
    let dir = tempfile::tempdir().unwrap();
    let log = AuditLog::new(dir.path().join("state").join("audit.log"));

    for n in 0..3 {
        log.record(record(n));
    }

    assert_eq!(log.read(10).unwrap(), [record(0), record(1), record(2)]);
    assert_eq!(log.read(2).unwrap(), [record(1), record(2)]);
}

#[test]
fn a_full_log_is_rotated_and_the_oldest_files_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.log");
    let line = serde_json::to_vec(&record(0)).unwrap().len() as u64 + 1;
    // Two records a file, the current one and two rotated ones.
    let log = AuditLog::with_limits(&path, line * 2, 2);

    for n in 0..9 {
        log.record(record(n));
    }

    let kept: Vec<i64> = log
        .read(100)
        .unwrap()
        .iter()
        .map(|r| r.time - 1_900_000_000)
        .collect();
    assert_eq!(kept, [4, 5, 6, 7, 8]);
    assert!(std::fs::metadata(&path).unwrap().len() <= line * 2);
    assert!(!dir.path().join("audit.log.3").exists());
}

#[test]
fn an_operator_gives_the_log_size_in_bytes_or_with_a_binary_suffix() {
    use keefast_agent::audit::parse_log_size;

    assert_eq!(parse_log_size("65536"), Ok(65536));
    assert_eq!(parse_log_size("512K"), Ok(512 << 10));
    assert_eq!(parse_log_size("16M"), Ok(16 << 20));
    assert_eq!(parse_log_size("16MiB"), Ok(16 << 20));
    assert_eq!(parse_log_size("1g"), Ok(1 << 30));
    for bad in ["", "M", "16 parsecs", "-1M", "1K", "99999999999G"] {
        assert!(parse_log_size(bad).is_err(), "{bad}");
    }
}

#[test]
fn actions_are_written_in_snake_case_and_unknown_ones_still_read() {
    let mut http = record(0);
    http.action = AuditAction::HttpRequest;
    let line = serde_json::to_value(&http).unwrap();
    assert_eq!(line["action"], "http_request");

    // A later keefast may log an action this one doesn't know; its records still read.
    let mut later = line.clone();
    later["action"] = "teleport".into();
    let read: AuditRecord = serde_json::from_value(later).unwrap();
    assert_eq!(read.action, AuditAction::Other);
}
