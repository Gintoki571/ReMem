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
