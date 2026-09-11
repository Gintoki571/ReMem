//! CLI surface for edge provenance (issue #15): RED-first.

use std::path::PathBuf;
use std::process::Command;

fn db(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("remem-edge-cli-{tag}-{}.db", std::process::id()));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{}", p.display(), suffix)));
    }
    p
}

fn run(db: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_remem"))
        .args(["--db", db.to_str().unwrap()])
        .args(args)
        .output()
        .expect("run remem")
}

fn ok(db: &std::path::Path, args: &[&str]) -> String {
    let out = run(db, args);
    assert!(
        out.status.success(),
        "remem {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn remember(db: &std::path::Path, kind: &str, text: &str, tags: &str) -> String {
    ok(db, &["remember", kind, text, "--tags", tags])
        .lines()
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn link_provenance_roundtrips_through_related() {
    let d = db("rel");
    let a = remember(&d, "fact", "postgres listens on 5432", "db");
    let b = remember(&d, "fact", "vacuum the postgres tables", "db");
    ok(
        &d,
        &[
            "link",
            &a,
            &b,
            "--rel",
            "SUPERSEDES",
            "--provenance",
            "correction-chain",
        ],
    );
    let related = ok(&d, &["related", &a]);
    assert!(
        related.contains("correction-chain"),
        "related must show provenance, got: {related}"
    );
    // default link stays manual
    let c = remember(&d, "fact", "index the postgres tables", "db");
    ok(&d, &["link", &a, &c]);
    let related = ok(&d, &["related", &a]);
    assert!(related.contains("manual"), "got: {related}");
}

#[test]
fn link_rejects_unknown_provenance() {
    let d = db("rej");
    let a = remember(&d, "fact", "alpha fact one", "t");
    let b = remember(&d, "fact", "beta fact two", "t");
    let out = run(&d, &["link", &a, &b, "--provenance", "llm-guess"]);
    assert!(!out.status.success(), "unknown provenance must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown provenance"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn correct_writes_correction_chain_edge() {
    let d = db("correct");
    let v1 = remember(&d, "fact", "deploy uses port 8080", "deploy");
    let v2 = ok(
        &d,
        &[
            "correct",
            &v1,
            "fact",
            "deploy uses port 9090",
            "--tags",
            "deploy",
        ],
    )
    .lines()
    .next()
    .unwrap()
    .to_string();
    assert_ne!(v1, v2);
    let chain = ok(&d, &["trace", &v2]);
    assert!(chain.contains(&v1) && chain.contains(&v2), "got: {chain}");
    let related = ok(&d, &["related", &v2]);
    assert!(
        related.contains("SUPERSEDES") && related.contains("correction-chain"),
        "got: {related}"
    );
}

#[test]
fn validate_flags_cross_topic_supersedes() {
    let d = db("validate");
    let pg = remember(&d, "fact", "postgres tuning notes", "db");
    let garden = remember(&d, "fact", "rose pruning guide", "garden");
    ok(&d, &["link", &pg, &garden, "--rel", "SUPERSEDES"]);
    let out = run(&d, &["validate"]);
    assert!(!out.status.success(), "validate must exit 1");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("suspect SUPERSEDES") && stdout.contains("no shared tags"),
        "got: {stdout}"
    );
}

#[test]
fn central_shows_provenance() {
    let d = db("central");
    let a = remember(&d, "fact", "postgres listens on 5432", "db");
    let b = remember(&d, "fact", "vacuum the postgres tables", "db");
    ok(
        &d,
        &[
            "link",
            &a,
            &b,
            "--rel",
            "SUPERSEDES",
            "--provenance",
            "correction-chain",
        ],
    );
    let central = ok(&d, &["central"]);
    assert!(
        central.contains("correction-chain"),
        "central must show provenance, got: {central}"
    );
}
