//! Regression tests for the 0.1.4 temporal and invalidation hunt (time.md,
//! rel.md, er.md, scale.md). Each test reproduces one reported behaviour.

use neocortex::{
    embed::HashEmbedder,
    engine::{Engine, EntityIn, RecallInput, RelationIn, RememberInput},
    temporal::{days_from_civil, Clock},
};
use std::sync::Arc;

/// 2026-10-23 12:00 UTC.
const NOW: i64 = 1_792_756_800;

fn engine() -> Engine {
    Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(Clock {
            now: NOW,
            tz_offset_secs: 0,
        })
}

/// Unix seconds at 00:00 UTC of a civil date.
fn day(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d) * 86_400
}

fn person(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some("person".into()),
        ..Default::default()
    }
}

fn org(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some("org".into()),
        ..Default::default()
    }
}

fn rel(src: &str, predicate: &str, dst: &str, exclusive: bool) -> RelationIn {
    RelationIn {
        source: src.into(),
        predicate: predicate.into(),
        target: dst.into(),
        exclusive: Some(exclusive),
        ..Default::default()
    }
}

fn memory(e: &Engine, content: &str, when: Option<&str>) -> neocortex::engine::RememberOutput {
    e.remember(RememberInput {
        content: content.into(),
        valid_from: when.map(Into::into),
        ..Default::default()
    })
    .unwrap()
}

fn sql_i64(e: &Engine, sql: &str) -> Option<i64> {
    e.db.sql().query_row(sql, [], |r| r.get(0)).unwrap()
}

// TM-02: --until on a memory must end the relation that only it evidences.
#[test]
fn tm02_until_ends_relation_that_only_the_memory_evidences() {
    let e = engine();
    let out = e
        .remember(RememberInput {
            content: "Kim was a contractor for Oscorp until 2021.".into(),
            valid_from: Some("2020-01-01".into()),
            valid_to: Some("2021-01-01".into()),
            entities: vec![person("Kim"), org("Oscorp")],
            relations: vec![rel("Kim", "works for", "Oscorp", false)],
            ..Default::default()
        })
        .unwrap();
    let rid = out.relations[0].id;
    let valid_to: Option<i64> = sql_i64(
        &e,
        &format!("SELECT valid_to FROM relations WHERE id={rid}"),
    );
    assert_eq!(valid_to, Some(day(2021, 1, 1)));
}

// TM-02: validate must report a live relation whose evidence has all ended.
#[test]
fn tm02_validate_flags_live_relation_with_only_ended_evidence() {
    let e = engine();
    e.remember(RememberInput {
        content: "Kim works for Oscorp.".into(),
        entities: vec![person("Kim"), org("Oscorp")],
        relations: vec![rel("Kim", "works for", "Oscorp", false)],
        ..Default::default()
    })
    .unwrap();
    assert!(e.validate().unwrap().is_empty());
    // A state written before the fix: the memory ended, the relation did not.
    e.db.sql()
        .execute(
            "UPDATE memories SET valid_to=?1 WHERE id=1",
            [day(2021, 1, 1)],
        )
        .unwrap();
    let problems = e.validate().unwrap();
    assert!(
        problems.iter().any(|p| p.contains("r1")),
        "validate missed the stale relation: {problems:?}"
    );
}

// ER-05: `invalidate m<id>` ends the relation that memory alone evidences.
#[test]
fn er05_invalidate_memory_ends_its_only_relation() {
    let e = engine();
    let out = e
        .remember(RememberInput {
            content: "Alice lives in Austin.".into(),
            valid_from: Some("2024-01-01".into()),
            entities: vec![person("Alice"), org("Austin")],
            relations: vec![rel("Alice", "lives in", "Austin", true)],
            ..Default::default()
        })
        .unwrap();
    let rid = out.relations[0].id;
    e.invalidate("m1", Some("2025-06-01")).unwrap();
    let valid_to = sql_i64(
        &e,
        &format!("SELECT valid_to FROM relations WHERE id={rid}"),
    );
    assert_eq!(valid_to, Some(day(2025, 6, 1)));
}

// ER-06: an equal-start exclusive replacement leaves the old memory ended.
#[test]
fn er06_equal_start_replacement_ends_the_old_memory() {
    let e = engine();
    for (content, city) in [
        ("Alice lives in Austin.", "Austin"),
        ("Alice lives in Denver.", "Denver"),
    ] {
        e.remember(RememberInput {
            content: content.into(),
            valid_from: Some("2025-01-01".into()),
            entities: vec![person("Alice"), org(city)],
            relations: vec![rel("Alice", "lives in", city, true)],
            ..Default::default()
        })
        .unwrap();
    }
    let valid_to = sql_i64(&e, "SELECT valid_to FROM memories WHERE id=1");
    assert_eq!(
        valid_to,
        Some(day(2025, 1, 1)),
        "old memory still looks current"
    );
    let out = e
        .recall(&RecallInput {
            query: "where does Alice live".into(),
            ..Default::default()
        })
        .unwrap();
    let old = out
        .memories
        .iter()
        .find(|m| m.id == 1)
        .expect("m1 returned");
    assert!(old.ended, "old memory must be marked ended");
}

fn alice_history(e: &Engine) {
    let cities = [
        (
            "Alice lived in Austin.",
            "Austin",
            "2020-01-01",
            Some("2023-07-01"),
        ),
        (
            "Alice lived in Boston.",
            "Boston",
            "2023-07-01",
            Some("2025-01-01"),
        ),
        ("Alice lives in Denver.", "Denver", "2025-01-01", None),
    ];
    for (content, city, from, to) in cities {
        e.remember(RememberInput {
            content: content.into(),
            valid_from: Some(from.into()),
            valid_to: to.map(Into::into),
            entities: vec![person("Alice"), org(city)],
            relations: vec![rel("Alice", "lives in", city, true)],
            ..Default::default()
        })
        .unwrap();
    }
}

// TM-03: a memory valid during the window is kept even if it started before it.
#[test]
fn tm03_since_until_keeps_memories_overlapping_the_window() {
    let e = engine();
    alice_history(&e);
    let out = e
        .recall(&RecallInput {
            query: "Alice".into(),
            since: Some("2021-01-01".into()),
            until: Some("2021-12-31".into()),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<i64> = out.memories.iter().map(|m| m.id).collect();
    assert!(
        ids.contains(&1),
        "Austin overlaps 2021 but was dropped: {ids:?}"
    );
    assert!(!ids.contains(&2), "Boston starts after 2021: {ids:?}");
}

// TM-04: facts follow the same since/until window as memories.
#[test]
fn tm04_since_until_filters_facts() {
    let e = engine();
    alice_history(&e);
    let early = e
        .recall(&RecallInput {
            query: "Alice".into(),
            until: Some("2019-12-31".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        early.facts.is_empty(),
        "facts outside the window: {:?}",
        early.facts
    );
    let year = e
        .recall(&RecallInput {
            query: "Alice".into(),
            since: Some("2024-01-01".into()),
            until: Some("2024-12-31".into()),
            ..Default::default()
        })
        .unwrap();
    let targets: Vec<&str> = year.facts.iter().map(|f| f.target.as_str()).collect();
    assert!(
        !targets.contains(&"Denver"),
        "Denver starts in 2025: {targets:?}"
    );
    assert!(
        targets.contains(&"Boston"),
        "Boston overlaps 2024: {targets:?}"
    );
}

// TM-05: a time window with no overlapping memory is reported, not silently ignored.
#[test]
fn tm05_window_without_overlapping_memories_is_flagged() {
    let e = engine();
    alice_history(&e);
    let empty = e
        .recall(&RecallInput {
            query: "Alice in 2019".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(empty.window.is_some());
    assert!(empty.window_empty, "2019 has no memory but was not flagged");
    let hit = e
        .recall(&RecallInput {
            query: "Alice in 2021".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(!hit.window_empty, "2021 overlaps Austin");
}

// TM-06: a second invalidate must not move the end later.
#[test]
fn tm06_reinvalidating_keeps_the_earliest_end() {
    let e = engine();
    let out = e
        .remember(RememberInput {
            content: "Judy works at Initrode.".into(),
            valid_from: Some("2022-01-01".into()),
            entities: vec![person("Judy"), org("Initrode")],
            relations: vec![rel("Judy", "works at", "Initrode", true)],
            ..Default::default()
        })
        .unwrap();
    let rid = out.relations[0].id;
    e.invalidate(&format!("r{rid}"), Some("2024-06-01"))
        .unwrap();
    let again = e.invalidate(&format!("r{rid}"), None).unwrap();
    assert!(
        again.changed[0].contains("already ended"),
        "{:?}",
        again.changed
    );
    let valid_to = sql_i64(
        &e,
        &format!("SELECT valid_to FROM relations WHERE id={rid}"),
    );
    assert_eq!(valid_to, Some(day(2024, 6, 1)));
    e.invalidate("m1", Some("2024-06-01")).unwrap();
    e.invalidate("m1", None).unwrap();
    assert_eq!(
        sql_i64(&e, "SELECT valid_to FROM memories WHERE id=1"),
        Some(day(2024, 6, 1))
    );
}

// TM-07: "from X to Y" sets both ends instead of keeping only the start.
#[test]
fn tm07_when_range_sets_valid_from_and_valid_to() {
    let e = engine();
    let out = memory(&e, "Interval test", Some("from 2024-01 to 2024-03"));
    assert_eq!(out.id, 1);
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=1"),
        Some(day(2024, 1, 1))
    );
    assert_eq!(
        sql_i64(&e, "SELECT valid_to FROM memories WHERE id=1"),
        Some(day(2024, 4, 1))
    );
    memory(
        &e,
        "Between test",
        Some("between 2024-05-01 and 2024-05-31"),
    );
    assert_eq!(
        sql_i64(&e, "SELECT valid_to FROM memories WHERE id=2"),
        Some(day(2024, 6, 1))
    );
}

// TM-08: "N weeks/months ago" stores that exact day, not the start of a window.
#[test]
fn tm08_weeks_and_months_ago_store_the_exact_day() {
    let e = engine();
    memory(&e, "two weeks", Some("two weeks ago"));
    memory(&e, "two months", Some("2 months ago"));
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=1"),
        Some(day(2026, 10, 9))
    );
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=2"),
        Some(day(2026, 8, 23))
    );
}

// TM-09: --when takes the same "last <month>" phrase recall takes, and the
// error names the accepted forms.
#[test]
fn tm09_when_accepts_last_month_name_and_error_lists_forms() {
    let e = engine();
    memory(&e, "march", Some("last March"));
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=1"),
        Some(day(2026, 3, 1))
    );
    let err = e
        .remember(RememberInput {
            content: "bad".into(),
            valid_from: Some("03/04/2025x".into()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("March 2025") && err.contains("3 days ago"),
        "{err}"
    );
}

// TM-01: a date written in a text-only memory becomes its valid_from.
#[test]
fn tm01_text_date_becomes_valid_from() {
    let e = engine();
    let carol = memory(&e, "Carol got engaged on March 5, 2025.", None);
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=1"),
        Some(day(2025, 3, 5))
    );
    assert!(carol.when_inferred.is_some_and(|t| t.contains("March 5")));
    memory(&e, "Nia was hired in January 2019.", None);
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=2"),
        Some(day(2019, 1, 1))
    );
    memory(&e, "I got the job yesterday at Initech.", None);
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=3"),
        Some(day(2026, 10, 22))
    );
    memory(&e, "Phoenix ships on 2027-06-15.", None);
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=4"),
        Some(day(2027, 6, 15))
    );
}

// TM-01: several dates in one text are ambiguous, so nothing is inferred.
#[test]
fn tm01_ambiguous_text_dates_are_not_inferred() {
    let e = engine();
    let out = memory(&e, "Met on 2020-01-01 and again on 2021-02-02.", None);
    assert!(out.when_inferred.is_none());
    assert_eq!(
        sql_i64(&e, "SELECT valid_from FROM memories WHERE id=1"),
        Some(NOW)
    );
}

// ER-08: forgetting the only source retracts its relation; it gets no end date.
#[test]
fn er08_forget_retracts_relation_without_end_date() {
    let e = engine();
    let out = e
        .remember(RememberInput {
            content: "Maya Lin manages the Atlas project.".into(),
            valid_from: Some("2025-02-01".into()),
            entities: vec![person("Maya Lin"), org("Atlas")],
            relations: vec![rel("Maya Lin", "manages", "Atlas", false)],
            ..Default::default()
        })
        .unwrap();
    let rid = out.relations[0].id;
    e.forget("m1", false).unwrap();
    let row: (Option<i64>, Option<i64>) =
        e.db.sql()
            .query_row(
                &format!("SELECT valid_to, expired_at FROM relations WHERE id={rid}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(row.0, None, "forget must not invent an end date");
    assert!(row.1.is_some(), "relation must be retracted (expired_at)");
}

// scale ER-06: the budget holds except for the single top item, which is flagged.
#[test]
fn scale_er06_over_budget_is_flagged() {
    let e = engine();
    for i in 0..3 {
        memory(
            &e,
            &format!("Hub fact {i}: the service depends on the queue and retries."),
            None,
        );
    }
    let tight = e
        .recall(&RecallInput {
            query: "service queue".into(),
            budget: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(tight.memories.len(), 1);
    assert!(tight.tokens_used > 1);
    assert!(tight.over_budget);
    let roomy = e
        .recall(&RecallInput {
            query: "service queue".into(),
            budget: Some(10_000),
            ..Default::default()
        })
        .unwrap();
    assert!(!roomy.over_budget);
}

// scale ER-07: an exact duplicate reports no near-duplicate of itself.
#[test]
fn scale_er07_exact_duplicate_has_no_near_duplicate() {
    let e = engine();
    memory(&e, "The deployment uses SQLite for durable memory.", None);
    let again = memory(&e, "The deployment uses SQLite for durable memory.", None);
    assert_eq!(again.duplicate_of, Some(1));
    assert!(again.near_duplicate.is_none(), "{:?}", again.near_duplicate);
}
