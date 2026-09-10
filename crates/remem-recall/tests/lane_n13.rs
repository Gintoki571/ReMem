//! Lane N13 (issue #13): recall CLI/env wiring for recency + score floor.
//! Runs the real binary against isolated db files.

use std::path::{Path, PathBuf};
use std::process::Command;

fn db(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "remem-lane-n13-{name}-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for s in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{s}", p.display())));
    }
    p
}

fn cleanup(db: &Path) {
    for s in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{s}", db.display())));
    }
}

fn run(db: &Path, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_remem"));
    c.args(["--db", db.to_str().unwrap()]).args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    // Never inherit ambient wiring env from the developer shell.
    for k in [
        "REMEM_NO_RECENCY",
        "REMEM_HALF_LIFE_DAYS",
        "REMEM_COSINE_FLOOR",
    ] {
        if !envs.iter().any(|(e, _)| *e == k) {
            c.env_remove(k);
        }
    }
    c.output().expect("run remem")
}

fn ok(db: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let out = run(db, args, envs);
    assert!(
        out.status.success(),
        "args {args:?} env {envs:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Seed one memory with event time 600 s ago; returns its id.
fn seed(db: &Path) -> String {
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 600;
    ok(
        db,
        &[
            "remember",
            "fact",
            "quarry ledger tracks basalt shipments",
            "--occurred-at",
            &when.to_string(),
        ],
        &[],
    )
    .lines()
    .next()
    .unwrap()
    .to_string()
}

fn recall(db: &Path, extra: &[&str], envs: &[(&str, &str)]) -> String {
    let mut args = vec!["recall", "quarry", "basalt", "ledger"];
    args.extend(extra);
    ok(db, &args, envs)
}

#[test]
fn help_lists_n13_flags() {
    let d = db("help");
    let out = ok(&d, &["recall", "--help"], &[]);
    for flag in ["--no-recency", "--half-life-days", "--min-score"] {
        assert!(out.contains(flag), "help missing {flag}:\n{out}");
    }
    cleanup(&d);
}

#[test]
fn no_recency_flag_drops_recent_reason() {
    let d = db("flag");
    let id = seed(&d);
    let plain = recall(&d, &[], &[]);
    assert!(plain.contains(&id), "seed memory not recalled: {plain}");
    assert!(
        plain.contains("recent"),
        "expected a `recent` reason: {plain}"
    );
    let off = recall(&d, &["--no-recency"], &[]);
    assert!(off.contains(&id), "memory lost with --no-recency: {off}");
    assert!(
        !off.contains("recent"),
        "recent leaked through --no-recency: {off}"
    );
    cleanup(&d);
}

#[test]
fn no_recency_env_matches_flag() {
    let d = db("env");
    let id = seed(&d);
    let out = recall(&d, &[], &[("REMEM_NO_RECENCY", "true")]);
    assert!(out.contains(&id), "memory lost with env: {out}");
    assert!(!out.contains("recent"), "recent leaked through env: {out}");
    cleanup(&d);
}

#[test]
fn tiny_half_life_env_drops_recent_and_flag_wins() {
    let d = db("half");
    let id = seed(&d);
    // 600 s old vs 0.001 d (86 s) half-life: recency ~0.008, no `recent`.
    let out = recall(&d, &[], &[("REMEM_HALF_LIFE_DAYS", "0.001")]);
    assert!(out.contains(&id), "memory lost with tiny half-life: {out}");
    assert!(
        !out.contains("recent"),
        "recent survived tiny half-life: {out}"
    );
    // CLI flag beats env: back to 30 d half-life restores `recent`.
    let out = recall(
        &d,
        &["--half-life-days", "30"],
        &[("REMEM_HALF_LIFE_DAYS", "0.001")],
    );
    assert!(out.contains("recent"), "flag did not beat env: {out}");
    cleanup(&d);
}

#[test]
fn cosine_floor_env_narrows_recall() {
    let d = db("floor");
    let id = seed(&d);
    let plain = recall(&d, &[], &[]);
    assert!(plain.contains(&id), "seed memory not recalled: {plain}");
    let out = recall(&d, &[], &[("REMEM_COSINE_FLOOR", "99")]);
    assert!(!out.contains(&id), "floor 99 still returned the hit: {out}");
    cleanup(&d);
}

#[test]
fn invalid_half_life_is_rejected() {
    let d = db("bad");
    let _ = seed(&d);
    for bad in ["0", "-3", "nan"] {
        // `=` form: a bare "-3" would parse as a flag, not a value.
        let flag = format!("--half-life-days={bad}");
        let out = run(&d, &["recall", "quarry", &flag], &[]);
        assert!(!out.status.success(), "--half-life-days {bad} accepted");
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(err.contains("half-life"), "stderr names nothing: {err}");
    }
    cleanup(&d);
}
