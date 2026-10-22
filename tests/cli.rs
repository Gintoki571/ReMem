//! CLI surface: version, help, usage errors and exit codes.
//! Commands that need the engine are `#[ignore]` until phase 2 lands.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_neocortex"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run neocortex")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn temp_db() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("neocortex-cli-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("memory.db")
}

#[test]
fn version_flag_prints_the_crate_version() {
    let o = run(&["--version"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        stdout(&o).trim(),
        format!("neocortex {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_lists_the_commands() {
    for arg in ["help", "--help"] {
        let o = run(&[arg]);
        assert!(o.status.success(), "{arg}: {}", stderr(&o));
        let text = stdout(&o);
        assert!(text.contains("Usage: neocortex"), "{text}");
        for cmd in [
            "remember",
            "recall",
            "entity",
            "invalidate",
            "forget",
            "mcp",
        ] {
            assert!(text.contains(cmd), "help misses {cmd}: {text}");
        }
    }
}

#[test]
fn unknown_command_is_a_usage_error() {
    let o = run(&["frobnicate"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("frobnicate"));
    assert!(stdout(&o).is_empty());
}

#[test]
fn flag_without_value_is_a_usage_error() {
    let o = run(&["recall", "anything", "--k"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("--k"));
}

#[test]
fn remember_without_content_is_a_usage_error() {
    let o = run(&["remember"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).to_lowercase().contains("content"));
}

// ------------------------------------------------------------ phase 2

fn memory_cmd(db: &Path, args: &[&str]) -> Output {
    bin()
        .env("NEOCORTEX_DB", db)
        .env("NEOCORTEX_EMBEDDER", "hash")
        .args(args)
        .output()
        .expect("run neocortex")
}

#[test]
#[ignore = "needs phase-2 engine"]
fn remember_then_recall_from_the_cli() {
    let db = temp_db();
    let o = memory_cmd(
        &db,
        &[
            "remember",
            "Mom",
            "moved",
            "from",
            "Delhi",
            "to",
            "Pune",
            "on",
            "2026-09-14.",
            "--kind",
            "event",
        ],
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains('m'), "{}", stdout(&o));

    let o = memory_cmd(&db, &["recall", "where", "does", "Mom", "live"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("Pune"), "{}", stdout(&o));

    let o = memory_cmd(&db, &["--json", "recall", "Mom"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("json output");
    assert!(v["memories"].as_array().is_some(), "{v}");
}

#[test]
#[ignore = "needs phase-2 engine"]
fn remember_reads_json_from_stdin() {
    use std::io::Write;
    let db = temp_db();
    let mut child = bin()
        .env("NEOCORTEX_DB", &db)
        .env("NEOCORTEX_EMBEDDER", "hash")
        .args(["remember", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"content":"The staging box runs Debian 13.","kind":"fact"}"#)
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
#[ignore = "needs phase-2 engine"]
fn invalidate_forget_and_stats_from_the_cli() {
    let db = temp_db();
    let o = memory_cmd(&db, &["remember", "The deploy runs on Fridays."]);
    assert!(o.status.success(), "{}", stderr(&o));

    let o = memory_cmd(&db, &["invalidate", "m1", "--at", "2026-10-01"]);
    assert!(o.status.success(), "{}", stderr(&o));

    let o = memory_cmd(&db, &["forget", "m1", "--hard"]);
    assert!(o.status.success(), "{}", stderr(&o));

    let o = memory_cmd(&db, &["stats"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("memories"), "{}", stdout(&o));
}
