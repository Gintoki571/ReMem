use neocortex::{
    embed::HashEmbedder,
    engine::{Engine, EntityIn, RelationIn, RememberInput},
    temporal::Clock,
};
use std::sync::Arc;

const T0: i64 = 1_700_000_000;
const T1: i64 = T0 + 86_400;

fn engine() -> Engine {
    Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(Clock {
            now: T1,
            tz_offset_secs: 0,
        })
}

fn input(content: &str, from: i64, relation: RelationIn) -> RememberInput {
    RememberInput {
        content: content.into(),
        valid_from: Some(from.to_string()),
        entities: vec![
            EntityIn {
                name: "Bob".into(),
                ..Default::default()
            },
            EntityIn {
                name: "Acme".into(),
                ..Default::default()
            },
            EntityIn {
                name: "Globex".into(),
                ..Default::default()
            },
        ],
        relations: vec![relation],
        ..Default::default()
    }
}

fn relation(target: &str) -> RelationIn {
    RelationIn {
        source: "Bob".into(),
        predicate: "works at".into(),
        target: target.into(),
        ..Default::default()
    }
}

#[test]
fn seeded_single_value_ends_old_relation_and_records_ended_by() {
    let e = engine();
    e.remember(input("Bob works at Acme", T0, relation("Acme")))
        .unwrap();
    let out = e
        .remember(input("Bob works at Globex", T1, relation("Globex")))
        .unwrap();
    assert_eq!(out.invalidated_relations.len(), 1);
    let ended_by: i64 =
        e.db.sql()
            .query_row(
                "SELECT ended_by FROM relations WHERE valid_to=?1",
                [T1],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(ended_by, out.relations[0].id);
}

#[test]
fn relation_json_missing_exclusive_is_none_and_false_is_explicit() {
    let missing: RelationIn =
        serde_json::from_str(r#"{"source":"a","predicate":"likes","target":"b"}"#).unwrap();
    assert_eq!(missing.exclusive, None);
    let false_value: RelationIn = serde_json::from_str(
        r#"{"source":"a","predicate":"likes","target":"b","exclusive":false}"#,
    )
    .unwrap();
    assert_eq!(false_value.exclusive, Some(false));
}

#[test]
fn negative_relation_ends_positive_without_merging_evidence() {
    let e = engine();
    let positive = RelationIn {
        source: "Bob".into(),
        predicate: "likes".into(),
        target: "Jazz".into(),
        ..Default::default()
    };
    let negative = RelationIn {
        fact: Some("Bob no longer likes Jazz".into()),
        negated: Some(true),
        ..positive.clone()
    };
    e.remember(RememberInput {
        content: "Bob likes Jazz".into(),
        valid_from: Some(T0.to_string()),
        entities: vec![EntityIn {
            name: "Bob".into(),
            ..Default::default()
        }],
        relations: vec![positive],
        ..Default::default()
    })
    .unwrap();
    e.remember(RememberInput {
        content: "Bob no longer likes Jazz".into(),
        valid_from: Some(T1.to_string()),
        entities: vec![EntityIn {
            name: "Bob".into(),
            ..Default::default()
        }],
        relations: vec![negative],
        ..Default::default()
    })
    .unwrap();
    let rows: Vec<(i64, i64, Option<i64>)> =
        e.db.sql()
            .prepare("SELECT polarity,evidence_count,ended_by FROM relations ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 1);
    assert_eq!(rows[0].1, 1);
    assert_eq!(rows[0].2, Some(2));
    assert_eq!(rows[1].0, 0);
}

#[test]
fn nonexclusive_conflict_is_reported_but_false_suppresses_it() {
    let e = engine();
    let mut first = relation("Acme");
    first.predicate = "likes".into();
    e.remember(input("Bob likes Acme", T0, first)).unwrap();
    let mut second = relation("Globex");
    second.predicate = "likes".into();
    let out = e.remember(input("Bob likes Globex", T1, second)).unwrap();
    assert_eq!(out.conflicts.len(), 1);
    let mut keep = relation("Globex");
    keep.predicate = "likes".into();
    keep.exclusive = Some(false);
    let out = e
        .remember(input("Bob also likes Globex", T1 + 1, keep))
        .unwrap();
    assert!(out.conflicts.is_empty());
}

#[test]
fn exclusive_false_overrides_seeded_predicate_for_this_write() {
    let e = engine();
    e.remember(input("Bob works at Acme", T0, relation("Acme")))
        .unwrap();
    let mut keep = relation("Globex");
    keep.exclusive = Some(false);
    let out = e
        .remember(input("Bob also works at Globex", T1, keep))
        .unwrap();
    assert!(out.invalidated_relations.is_empty());
    assert_eq!(
        e.db.sql()
            .query_row(
                "SELECT count(*) FROM relations WHERE expired_at IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    assert!(out.conflicts.is_empty());
}
