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

fn likes(content: &str, from: i64, negated: Option<bool>) -> RememberInput {
    RememberInput {
        content: content.into(),
        valid_from: Some(from.to_string()),
        relations: vec![RelationIn {
            source: "Alice".into(),
            predicate: "likes".into(),
            target: "jazz".into(),
            negated,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn live(e: &Engine) -> Vec<(i64, i64)> {
    e.db.sql()
        .prepare("SELECT id,polarity FROM relations WHERE valid_to IS NULL AND expired_at IS NULL ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn inferred_negation_ends_positive_and_flip_back_ends_negative() {
    let e = engine();
    let pos = e.remember(likes("Alice likes jazz.", T0, None)).unwrap();
    let neg = e
        .remember(likes("Alice no longer likes jazz.", T0 + 100, None))
        .unwrap();
    assert_ne!(
        neg.relations[0].id, pos.relations[0].id,
        "negation must not merge"
    );
    assert_eq!(live(&e), vec![(neg.relations[0].id, 0)]);
    let again = e
        .remember(likes("Alice likes jazz again.", T0 + 200, None))
        .unwrap();
    assert_eq!(live(&e), vec![(again.relations[0].id, 1)]);
    assert_eq!(again.invalidated_relations, vec![neg.relations[0].id]);
}

#[test]
fn backfilled_reassertion_adds_evidence_to_the_ended_relation() {
    let e = engine();
    let austin = |content: &str, from: i64| RememberInput {
        content: content.into(),
        valid_from: Some(from.to_string()),
        relations: vec![RelationIn {
            source: "Alice".into(),
            predicate: "lives in".into(),
            target: "Austin".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let first = e.remember(austin("Alice lives in Austin.", T0)).unwrap();
    let mut denver = austin("Alice moved to Denver.", T1);
    denver.relations[0].target = "Denver".into();
    let moved = e.remember(denver).unwrap();
    assert_eq!(moved.invalidated_relations, vec![first.relations[0].id]);
    let again = e
        .remember(austin("Alice lived in Austin back then.", T0))
        .unwrap();
    assert_eq!(again.relations[0].id, first.relations[0].id);
    let (count, to, by): (i64, Option<i64>, Option<i64>) =
        e.db.sql()
            .query_row(
                "SELECT evidence_count,valid_to,ended_by FROM relations WHERE id=?1",
                [first.relations[0].id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    assert_eq!((count, to, by), (2, Some(T1), Some(moved.relations[0].id)));
    // The entity card says what replaced it.
    let card = e.entity("Alice", 20).unwrap();
    let text = neocortex::render::render_entity(&card, &|t| t.to_string());
    assert!(
        text.contains(&format!("replaced by r{}", moved.relations[0].id)),
        "{text}"
    );
}

#[test]
fn ended_by_points_at_the_relation_that_ended_it_in_multi_relation_memories() {
    let e = engine();
    let rel = |p: &str, t: &str| RelationIn {
        source: "Bob".into(),
        predicate: p.into(),
        target: t.into(),
        ..Default::default()
    };
    let first = e
        .remember(RememberInput {
            content: "Bob works at Acme and lives in Austin.".into(),
            valid_from: Some(T0.to_string()),
            relations: vec![rel("works at", "Acme"), rel("lives in", "Austin")],
            ..Default::default()
        })
        .unwrap();
    let second = e
        .remember(RememberInput {
            content: "Bob works at Globex and lives in Denver.".into(),
            valid_from: Some(T1.to_string()),
            relations: vec![rel("works at", "Globex"), rel("lives in", "Denver")],
            ..Default::default()
        })
        .unwrap();
    for i in 0..2 {
        let by: Option<i64> =
            e.db.sql()
                .query_row(
                    "SELECT ended_by FROM relations WHERE id=?1",
                    [first.relations[i].id],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(by, Some(second.relations[i].id));
    }
}

#[test]
fn current_values_marker_only_for_several_current_values() {
    let e = engine();
    e.remember(likes("Alice likes jazz.", T0, None)).unwrap();
    let card = e.entity("Alice", 20).unwrap();
    let text = neocortex::render::render_entity(&card, &|t| t.to_string());
    assert!(!text.contains("current]"), "{text}");
    let mut hiking = likes("Alice likes hiking.", T0 + 1, None);
    hiking.relations[0].target = "hiking".into();
    e.remember(hiking).unwrap();
    let card = e.entity("Alice", 20).unwrap();
    let text = neocortex::render::render_entity(&card, &|t| t.to_string());
    assert!(text.contains("[1 of 2 current]"), "{text}");
}
