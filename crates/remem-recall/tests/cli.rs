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
    .lines()
    .next()
    .unwrap()
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
    .lines()
    .next()
    .unwrap()
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

    let a = run(&["remember", "fact", "alpha"])
        .1
        .lines()
        .next()
        .unwrap()
        .to_string();
    let b = run(&["remember", "fact", "beta"])
        .1
        .lines()
        .next()
        .unwrap()
        .to_string();

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
    .lines()
    .next()
    .unwrap()
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
    .lines()
    .next()
    .unwrap()
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
    .lines()
    .next()
    .unwrap()
    .to_string();

    let hits = remem(
        &db,
        &["recall", "budget", "keyword", "--max-chars", "64", "--json"],
    );
    assert!(!hits.contains(&long), "over-budget hit leaked: {hits}");
    assert!(hits.contains(&short), "fitting hit lost: {hits}");

    // Nothing fits: the top hit still comes back whole.
    let hits = remem(
        &db,
        &["recall", "budget", "keyword", "--max-chars", "1", "--json"],
    );
    assert!(hits.contains(&short), "top hit missing: {hits}");

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// `recall --min-score F` drops junk; omitting it keeps the old behaviour.
#[test]
fn cli_recall_min_score() {
    let db = std::env::temp_dir().join(format!("remem-cli-minscore-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);

    let id = remem(
        &db,
        &["remember", "fact", "staging postgres listens on port 5432"],
    )
    .lines()
    .next()
    .unwrap()
    .to_string();

    let hits = remem(&db, &["recall", "staging", "postgres", "port", "--json"]);
    assert!(hits.contains(&id), "default floor must change nothing");
    let top = serde_json::from_str::<Vec<serde_json::Value>>(&hits).unwrap()[0]["score"]
        .as_f64()
        .unwrap();

    let kept = remem(
        &db,
        &[
            "recall",
            "staging",
            "postgres",
            "port",
            "--json",
            "--min-score",
            &format!("{}", top / 2.0),
        ],
    );
    assert!(kept.contains(&id), "hit above the floor lost: {kept}");

    let empty = remem(
        &db,
        &[
            "recall",
            "staging",
            "postgres",
            "port",
            "--json",
            "--min-score",
            &format!("{}", top * 2.0),
        ],
    );
    assert!(
        serde_json::from_str::<Vec<serde_json::Value>>(&empty)
            .unwrap()
            .is_empty(),
        "floor above top hit must bail: {empty}"
    );

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// `remember` probes the new embedding against stored ones BEFORE inserting
/// and prints a `similar: [id dist, ...]` line when the writer is re-saving a
/// paraphrase. Distances are embedder-dependent (stub vs real BERT differ), so
/// these assertions stay structural: the line appears, names the stored id,
/// and the second id in it is the earlier paraphrase row.
#[test]
fn cli_remember_reports_similar_for_paraphrase_only() {
    let db = std::env::temp_dir().join(format!("remem-cli-similar-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let base = "the deploy script uses rsync over ssh to copy artifacts to the bastion host";
    // same text + one appended word; distinct topics that share no framing
    let para = "the deploy script uses rsync over ssh to copy artifacts to the bastion host today";
    let other = "banana bread recipe needs three ripe bananas and a handful of walnuts";

    let first = remem(&db, &["remember", "fact", base])
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert!(
        !first.contains("similar"),
        "empty store must stay silent: {first}"
    );

    let out = remem(&db, &["remember", "fact", para]);
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines.len(), 2, "want id + similar line: {out}");
    assert!(
        !lines[0].contains("similar"),
        "id line must stay bare: {out}"
    );
    let similar = lines[1];
    assert!(
        similar.starts_with("similar: [") && similar.ends_with(']'),
        "similar line shape: {out}"
    );
    assert!(
        similar.contains(&first),
        "similar must name the stored id: {out}"
    );

    let third = remem(&db, &["remember", "fact", other]);
    assert!(
        !third.contains("similar"),
        "distinct memory must stay silent: {third}"
    );

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// An exact re-save dedups on content_hash: the original id comes back and the
/// `similar` line stays off (the probe would otherwise match the stored copy).
#[test]
fn cli_exact_remember_reprint_has_no_similar_line() {
    let db = std::env::temp_dir().join(format!("remem-cli-similar-dup-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let text = "the one and only deploy script note";
    let first = remem(&db, &["remember", "fact", text])
        .lines()
        .next()
        .unwrap()
        .to_string();
    let again = remem(&db, &["remember", "fact", text]);
    assert_eq!(again.trim(), first, "exact dup must return one bare id");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}

/// forget (soft), related (Memory-only, --rel filter), central (PageRank
/// scored), path (unreachable = empty). Unknown ids are empty/false, not
/// errors, mirroring the MCP tools.
#[test]
fn cli_forget_related_central_and_path() {
    let db = std::env::temp_dir().join(format!("remem-cli-graph-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let mk = |text: &str| {
        remem(&db, &["remember", "fact", text])
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    let a = mk("node alpha");
    let b = mk("node beta");
    let c = mk("node gamma");
    let d = mk("isolated delta");
    remem(&db, &["link", &a, &b, "--rel", "SUPERSEDES"]);
    remem(&db, &["link", &b, &c, "--rel", "RELATES_TO"]);

    // related: both directions, sorted by id, Memory-only (hubs stay out)
    let mut want: Vec<String> = vec![format!("{a}  SUPERSEDES"), format!("{c}  RELATES_TO")];
    want.sort();
    let got: Vec<String> = remem(&db, &["related", &b])
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(got, want, "related both directions");

    let filtered: Vec<String> = remem(&db, &["related", &b, "--rel", "SUPERSEDES"])
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(filtered, vec![format!("{a}  SUPERSEDES")], "--rel filter");
    assert_eq!(remem(&db, &["related", &a]), format!("{b}  SUPERSEDES\n"));

    // path follows edges; delta is unreachable
    let hops = remem(&db, &["path", &a, &c]);
    let hops: Vec<&str> = hops.lines().collect();
    assert_eq!(hops, vec![a.as_str(), b.as_str(), c.as_str()], "a->c path");
    assert_eq!(remem(&db, &["path", &a, &d]), "", "unreachable is empty");
    assert_eq!(remem(&db, &["path", &a, &b]).lines().next().unwrap(), a);

    // central: ranked by PageRank, --limit caps the output
    let all = remem(&db, &["central"]);
    let lines: Vec<&str> = all.lines().collect();
    assert_eq!(lines.len(), 4, "every memory ranked: {all}");
    let scores: Vec<f64> = lines
        .iter()
        .map(|l| l.split_whitespace().nth(1).unwrap().parse().unwrap())
        .collect();
    assert!(
        scores.windows(2).all(|w| w[0] >= w[1]),
        "must be sorted desc: {all}"
    );
    assert_eq!(
        remem(&db, &["central", "--limit", "1"]).lines().count(),
        1,
        "--limit 1"
    );
    assert_eq!(remem(&db, &["central", "--limit", "0"]), "", "--limit 0");

    // forget is soft: gone from list/recall/related, and reversible-looking
    assert!(remem(&db, &["forget", &b]).contains(&b));
    let listing = remem(&db, &["list"]);
    assert!(
        !listing.contains(&b) && listing.contains(&a) && listing.contains(&c),
        "list after forget: {listing}"
    );
    assert_eq!(remem(&db, &["related", &a]), "", "forgotten neighbour gone");
    assert_eq!(
        remem(&db, &["path", &a, &c]),
        "",
        "forgotten node breaks the path"
    );
    assert_eq!(remem(&db, &["central", "--limit", "5"]).lines().count(), 3);
    // soft vs hard: purge actually removes the row
    let purged = remem(&db, &["purge", &c]);
    assert!(purged.contains(&c), "purge output: {purged}");

    // unknown ids: empty output, exit 0 (never an error)
    assert_eq!(remem(&db, &["related", "nope-1"]), "");
    assert_eq!(remem(&db, &["path", "nope-1", "nope-2"]), "");
    assert_eq!(remem(&db, &["forget", "nope-3"]), "");

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", db.display(), suffix)));
    }
}
