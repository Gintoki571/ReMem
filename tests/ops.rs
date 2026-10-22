//! Operational guarantees: JSON Lines export/import round-trips history, and
//! concurrent CLI writers plus a reader leave one consistent database.

use neocortex::embed::HashEmbedder;
use neocortex::engine::{Engine, EntityIn, RelationIn, RememberInput};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ------------------------------------------------------------ export/import

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "neocortex-ops-{}-{tag}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine(path: &Path) -> Engine {
    // Fixed UTC offset: the system default is the machine's local offset.
    let clock = neocortex::temporal::Clock {
        now: neocortex::temporal::Clock::system().now,
        tz_offset_secs: 0,
    };
    Engine::open(path, Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(clock)
}

fn person(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some("person".into()),
        description: Some("the user's mother".into()),
        aliases: vec!["mother".into()],
        same_as: None,
    }
}

fn place(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some("place".into()),
        ..EntityIn::default()
    }
}

fn lives_in(target: &str, from: &str, fact: &str) -> RelationIn {
    RelationIn {
        source: "Mom".into(),
        predicate: "lives in".into(),
        target: target.into(),
        fact: Some(fact.into()),
        exclusive: true,
        valid_from: Some(from.into()),
        valid_to: None,
    }
}

fn seed(engine: &Engine) {
    engine
        .remember(RememberInput {
            content: "Mom lives in Pune.".into(),
            kind: Some("fact".into()),
            tags: vec!["family".into(), "home".into()],
            importance: Some(0.8),
            scope: Some("personal".into()),
            source: Some("test".into()),
            valid_from: Some("2025-01-01".into()),
            entities: vec![person("Mom"), place("Pune")],
            relations: vec![lives_in("Pune", "2025-01-01", "Mom lives in Pune")],
            ..RememberInput::default()
        })
        .unwrap();
    engine
        .remember(RememberInput {
            content: "Mom lives in Denver.".into(),
            kind: Some("fact".into()),
            tags: vec!["family".into()],
            valid_from: Some("2026-01-01".into()),
            entities: vec![person("Mom"), place("Denver")],
            relations: vec![lives_in("Denver", "2026-01-01", "Mom lives in Denver")],
            ..RememberInput::default()
        })
        .unwrap();
    let friday = engine
        .remember(RememberInput {
            content: "The deploy day is Friday.".into(),
            kind: Some("fact".into()),
            tags: vec!["deploy".into()],
            valid_from: Some("2026-02-01".into()),
            ..RememberInput::default()
        })
        .unwrap();
    engine
        .remember(RememberInput {
            content: "The deploy day is Thursday.".into(),
            kind: Some("fact".into()),
            tags: vec!["deploy".into()],
            valid_from: Some("2026-03-01".into()),
            supersedes: vec![format!("m{}", friday.id)],
            ..RememberInput::default()
        })
        .unwrap();
    let stale = engine
        .remember(RememberInput {
            content: "The old staging password policy is retired.".into(),
            kind: Some("note".into()),
            valid_from: Some("2026-01-15".into()),
            ..RememberInput::default()
        })
        .unwrap();
    engine.forget(&format!("m{}", stale.id), false).unwrap();
}

fn live_contents(engine: &Engine) -> Vec<String> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT content FROM memories WHERE expired_at IS NULL ORDER BY content")
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

fn all_contents(engine: &Engine) -> Vec<String> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT content FROM memories ORDER BY content")
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

fn entity_rows(engine: &Engine) -> Vec<(String, String, String)> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT name, type, description FROM entities ORDER BY name")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn alias_rows(engine: &Engine) -> Vec<(String, String)> {
    let mut st = engine
        .db
        .sql()
        .prepare(
            "SELECT e.name, a.alias_norm FROM entity_aliases a
             JOIN entities e ON e.id = a.entity_id ORDER BY e.name, a.alias_norm",
        )
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

type FactRow = (String, String, String, String, i64, Option<i64>);

fn fact_rows(engine: &Engine) -> Vec<FactRow> {
    let mut st = engine
        .db
        .sql()
        .prepare(
            "SELECT s.name, p.canonical, d.name, r.fact, r.valid_from, r.valid_to
             FROM relations r
             JOIN entities s ON s.id = r.src
             JOIN entities d ON d.id = r.dst
             JOIN predicates p ON p.id = r.predicate_id
             ORDER BY s.name, p.canonical, d.name, r.valid_from",
        )
        .unwrap();
    st.query_map([], |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
        ))
    })
    .unwrap()
    .collect::<rusqlite::Result<Vec<_>>>()
    .unwrap()
}

#[test]
fn export_import_round_trips_history_entities_and_facts() {
    let dir = temp_dir("roundtrip");
    let source = engine(&dir.join("source.db"));
    seed(&source);
    let lines = dir.join("export.jsonl");

    let n = neocortex::export::export(&source, Some(&lines), false).unwrap();
    assert_eq!(n, 4, "forgotten memory stays out of the default export");
    let text = std::fs::read_to_string(&lines).unwrap();
    let records: Vec<neocortex::export::ExportRecord> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 4);
    assert_eq!(records[0].id.as_deref(), Some("m1"));
    assert!(!records[0].expired);
    let superseding = records.iter().find(|r| r.supersedes.len() == 1).unwrap();
    assert_eq!(superseding.supersedes, vec!["m3"]);
    let superseded = records
        .iter()
        .find(|r| r.id.as_deref() == Some("m3"))
        .unwrap();
    assert!(superseded.expired, "superseded rows travel as history");
    assert_eq!(
        superseded.valid_from.as_deref(),
        Some("2026-02-01T00:00:00Z")
    );
    // Each line is a RememberInput: the extra id/expired fields are ignored.
    let as_input: RememberInput = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(as_input.content, "Mom lives in Pune.");
    assert_eq!(as_input.importance, Some(0.8));
    assert_eq!(as_input.tags, vec!["family", "home"]);

    let target = engine(&dir.join("target.db"));
    let report = neocortex::export::import(&target, &lines).unwrap();
    assert_eq!(report.imported, 4);
    assert_eq!(report.superseded, 1);
    assert_eq!(report.forgotten, 0);

    assert_eq!(live_contents(&target), live_contents(&source));
    assert_eq!(entity_rows(&target), entity_rows(&source));
    assert_eq!(alias_rows(&target), alias_rows(&source));
    assert_eq!(fact_rows(&target), fact_rows(&source));

    let (superseded_by, expired_at, valid_to): (Option<i64>, Option<i64>, Option<i64>) = target
        .db
        .sql()
        .query_row(
            "SELECT superseded_by, expired_at, valid_to FROM memories
             WHERE content = 'The deploy day is Friday.'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(superseded_by.is_some(), "supersede chain survives");
    assert!(expired_at.is_some(), "superseded row is history, not live");
    assert!(valid_to.is_some());
    assert!(!all_contents(&target)
        .iter()
        .any(|c| c.contains("password policy")));
    assert!(target.validate().unwrap().is_empty());
}

#[test]
fn include_forgotten_keeps_retired_rows_retired() {
    let dir = temp_dir("forgotten");
    let source = engine(&dir.join("source.db"));
    seed(&source);
    let lines = dir.join("export.jsonl");
    assert_eq!(
        neocortex::export::export(&source, Some(&lines), true).unwrap(),
        5
    );

    let target = engine(&dir.join("target.db"));
    let report = neocortex::export::import(&target, &lines).unwrap();
    assert_eq!(report.imported, 5);
    assert_eq!(report.forgotten, 1);
    assert_eq!(all_contents(&target), all_contents(&source));
    assert_eq!(live_contents(&target), live_contents(&source));
    let expired: i64 = target
        .db
        .sql()
        .query_row(
            "SELECT count(*) FROM memories WHERE content LIKE '%password policy%' AND expired_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(expired, 1);
    assert!(target.validate().unwrap().is_empty());
}

// ------------------------------------------------------------ concurrency

fn cli(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_neocortex"))
        .env("NEOCORTEX_DB", db)
        .env("NEOCORTEX_EMBEDDER", "hash")
        .args(args)
        .output()
        .expect("run neocortex")
}

#[test]
fn concurrent_writers_and_reader_leave_one_consistent_database() {
    let dir = temp_dir("concurrency");
    let db = dir.join("memory.db");
    // Create the schema once, as an installed agent does on first use: opening
    // a *brand-new* file from many processes at once races on
    // `PRAGMA journal_mode=WAL`, which returns SQLITE_BUSY without consulting
    // the busy handler. Concurrent writes to an existing database are the
    // contract this test covers.
    drop(engine(&db));
    let stop = Arc::new(AtomicBool::new(false));

    let reader_db = db.clone();
    let reader_stop = Arc::clone(&stop);
    let reader = std::thread::spawn(move || {
        let mut reads = 0usize;
        while !reader_stop.load(Ordering::Relaxed) {
            let out = cli(&reader_db, &["recall", "concurrent writer", "--k", "5"]);
            assert!(
                out.status.success(),
                "recall failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            reads += 1;
        }
        reads
    });

    let writers: Vec<_> = (0..2)
        .map(|writer| {
            let db = db.clone();
            std::thread::spawn(move || {
                for i in 0..30 {
                    let content = format!(
                        "concurrent writer {writer} stored item {i} about the staging host"
                    );
                    let out = cli(&db, &["remember", &content]);
                    assert!(
                        out.status.success(),
                        "remember {writer}/{i} failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    assert!(reader.join().unwrap() > 0, "the reader never ran");

    let out = cli(&db, &["validate"]);
    assert!(
        out.status.success(),
        "validate: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let out = cli(&db, &["--json", "stats"]);
    let stats: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("stats json");
    assert_eq!(stats["memories"], 60, "{stats}");
    assert_eq!(stats["live_memories"], 60, "{stats}");
}
