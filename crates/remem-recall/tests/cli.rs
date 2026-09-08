//! CLI smoke test: run the real binary against an isolated db file.

use std::path::PathBuf;
use std::process::Command;

fn remem(db: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_remem"))
        .args(["--db", db.to_str().unwrap()])
        .args(args)
        .output()
        .expect("run remem");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn cli_roundtrip() {
    let db = std::env::temp_dir().join(format!("remem-cli-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);

    let id1 = remem(
        &db,
        &[
            "remember",
            "fact",
            "deploy scripts live in ops/",
            "--tags",
            "deploy,ops",
        ],
    )
    .trim()
    .to_string();
    let id2 = remem(
        &db,
        &[
            "remember",
            "preference",
            "prefer pnpm over npm",
            "--importance",
            "0.9",
        ],
    )
    .trim()
    .to_string();
    assert!(!id1.is_empty() && !id2.is_empty());

    remem(&db, &["link", &id1, &id2, "--rel", "SUPERSEDES"]);
    let listing = remem(&db, &["list"]);
    assert!(listing.contains("deploy scripts") && listing.contains("pnpm"));

    let recalled = remem(&db, &["recall", "deploy", "scripts", "--k", "3", "--json"]);
    assert!(recalled.contains(&id1), "recalled: {recalled}");

    let stats = remem(&db, &["stats"]);
    assert!(stats.contains("\"memories\": 2"), "stats: {stats}");
    assert!(stats.contains("\"edges\": 1"), "stats: {stats}");

    let bad = Command::new(env!("CARGO_BIN_EXE_remem"))
        .args(["--db", db.to_str().unwrap(), "remember", "nonsense", "text"])
        .output()
        .expect("run remem");
    assert!(!bad.status.success());

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// `validate` reports graph problems on stdout and exits 1; a healthy db exits 0.
#[test]
fn cli_validate_flags_orphans_and_passes_linked_memories() {
    let db = std::env::temp_dir().join(format!("remem-cli-validate-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_remem"))
            .args(["--db", db.to_str().unwrap()])
            .args(args)
            .output()
            .expect("run remem");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    };

    let a = run(&["remember", "fact", "alpha"]).1.trim().to_string();
    let b = run(&["remember", "fact", "beta"]).1.trim().to_string();

    let (code, stdout) = run(&["validate"]);
    assert_eq!(
        code,
        Some(1),
        "two unlinked memories must fail validation: {stdout}"
    );
    // memory ids are random UUIDs, so compare sorted, not in creation order
    let mut got: Vec<String> = stdout.lines().map(String::from).collect();
    got.sort();
    let mut want = vec![
        format!("orphan memory: {a} (no edges outside agent/session hubs)"),
        format!("orphan memory: {b} (no edges outside agent/session hubs)"),
    ];
    want.sort();
    assert_eq!(got, want, "stdout: {stdout}");

    run(&["link", &a, &b, "--rel", "RELATES_TO"]);
    let (code, stdout) = run(&["validate"]);
    assert_eq!(code, Some(0), "linked memories should pass: {stdout}");
    assert_eq!(stdout, "", "quiet on a healthy db");

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}
