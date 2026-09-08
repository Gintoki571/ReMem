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

/// Two clocks through the CLI: --occurred-at stores the event time,
/// --since/--until filter recall on it.
#[test]
fn cli_occurred_at_and_date_filters() {
    let db = std::env::temp_dir().join(format!("remem-cli-clocks-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let day = 86_400;
    let march = now - 40 * day;

    let old = remem(
        &db,
        &[
            "remember",
            "event",
            "march launch retro",
            "--occurred-at",
            &march.to_string(),
        ],
    )
    .trim()
    .to_string();
    let _recent = remem(&db, &["remember", "event", "march launch today"]);

    // list --json carries the event clock
    let listing = remem(&db, &["list", "--json"]);
    assert!(
        listing.contains(&format!("\"occurred_at\": {march}")),
        "list: {listing}"
    );

    // YYYY-MM-DD parses to unix seconds of that day (UTC midnight)
    remem(
        &db,
        &[
            "remember",
            "event",
            "iso dated event",
            "--occurred-at",
            "2024-03-01",
        ],
    );
    let listing = remem(&db, &["list", "--json"]);
    assert!(
        listing.contains(&format!("\"occurred_at\": {}", 1709251200)),
        "iso parse failed: {listing}"
    );

    // Recall restricted to the last 10 days must drop the 40-day-old event
    let hits = remem(
        &db,
        &[
            "recall",
            "march launch",
            "--since",
            &(now - 10 * day).to_string(),
            "--json",
        ],
    );
    assert!(
        !hits.contains(&old),
        "old event leaked into --since window: {hits}"
    );

    // A window around the event time finds it by event clock
    let hits = remem(
        &db,
        &[
            "recall",
            "march launch",
            "--since",
            &(march - day).to_string(),
            "--until",
            &(march + day).to_string(),
            "--json",
        ],
    );
    assert!(
        hits.contains(&old),
        "event missing from its own window: {hits}"
    );

    // Bad flag value fails loudly
    let bad = Command::new(env!("CARGO_BIN_EXE_remem"))
        .args([
            "--db",
            db.to_str().unwrap(),
            "remember",
            "event",
            "x",
            "--occurred-at",
            "not-a-date",
        ])
        .output()
        .expect("run remem");
    assert!(!bad.status.success());

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// `recall --max-chars N` packs results to a character budget.
#[test]
fn cli_recall_max_chars() {
    let db = std::env::temp_dir().join(format!("remem-cli-maxchars-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    // importance keeps the short memory top-ranked, so the long one is the
    // candidate that must be skipped rather than the always-kept top hit.
    let long = remem(
        &db,
        &[
            "remember",
            "fact",
            &format!("budget keyword {}", "z".repeat(200)),
            "--importance",
            "0.1",
        ],
    )
    .trim()
    .to_string();
    let short = remem(
        &db,
        &[
            "remember",
            "fact",
            "budget keyword short",
            "--importance",
            "0.9",
        ],
    )
    .trim()
    .to_string();

    let hits = remem(&db, &["recall", "budget", "keyword", "--max-chars", "64", "--json"]);
    assert!(!hits.contains(&long), "over-budget hit leaked: {hits}");
    assert!(hits.contains(&short), "fitting hit lost: {hits}");

    // Nothing fits: the top hit still comes back whole.
    let hits = remem(&db, &["recall", "budget", "keyword", "--max-chars", "1", "--json"]);
    assert!(hits.contains(&short), "top hit missing: {hits}");

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}
