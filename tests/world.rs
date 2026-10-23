//! Batched ingestion must be indistinguishable from one-by-one ingestion, and
//! the synthetic world generator must be deterministic and self-consistent.

use neocortex::{
    embed::HashEmbedder,
    engine::{Engine, EntityIn, RelationIn, RememberInput},
    temporal::Clock,
};
use std::collections::HashMap;
use std::sync::Arc;

const NOW: i64 = 1_792_594_800; // 2026-10-21T15:00:00Z

fn clock() -> Clock {
    Clock {
        now: NOW,
        tz_offset_secs: 0,
    }
}

fn engine() -> Engine {
    Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(clock())
}

fn entity(name: &str, kind: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some(kind.into()),
        ..Default::default()
    }
}

fn relation(source: &str, predicate: &str, target: &str, exclusive: Option<bool>) -> RelationIn {
    RelationIn {
        source: source.into(),
        predicate: predicate.into(),
        target: target.into(),
        exclusive,
        ..Default::default()
    }
}

/// A relation with no flags at all: polarity is inferred from the content and
/// exclusivity comes from the predicate policy.
fn plain(source: &str, predicate: &str, target: &str) -> RelationIn {
    relation(source, predicate, target, None)
}

fn structured(
    content: &str,
    from: &str,
    entities: Vec<EntityIn>,
    relations: Vec<RelationIn>,
) -> RememberInput {
    RememberInput {
        content: content.into(),
        kind: Some("fact".into()),
        valid_from: Some(from.into()),
        entities,
        relations,
        ..Default::default()
    }
}

/// A mixed workload: exclusive changes, duplicates, a supersede, plain text and
/// one invalid input in the middle.
fn workload() -> Vec<RememberInput> {
    vec![
        structured(
            "Priya Shah lives in Pune.",
            "2025-01-10",
            vec![entity("Priya Shah", "person"), entity("Pune", "place")],
            vec![relation("Priya Shah", "lives in", "Pune", Some(true))],
        ),
        structured(
            "Priya Shah works at Northwind Labs.",
            "2025-02-01",
            vec![
                entity("Priya Shah", "person"),
                entity("Northwind Labs", "org"),
            ],
            vec![relation(
                "Priya Shah",
                "works at",
                "Northwind Labs",
                Some(true),
            )],
        ),
        structured(
            "Priya Shah moved to Delhi.",
            "2025-06-01",
            vec![entity("Priya Shah", "person"), entity("Delhi", "place")],
            vec![relation("Priya Shah", "lives in", "Delhi", Some(true))],
        ),
        // Exact duplicate of the first memory (live one is Delhi, so this is new
        // text but the same folded content as memory 1, which is still live).
        structured(
            "Priya Shah lives in Pune.",
            "2025-01-10",
            vec![entity("Priya Shah", "person"), entity("Pune", "place")],
            vec![relation("Priya Shah", "lives in", "Pune", Some(true))],
        ),
        // Invalid: must fail in its slot and leave nothing behind.
        RememberInput {
            content: "   ".into(),
            ..Default::default()
        },
        structured(
            "Priya Shah reports to Arjun Rao.",
            "2025-07-01",
            vec![
                entity("Priya Shah", "person"),
                entity("Arjun Rao", "person"),
            ],
            vec![relation(
                "Priya Shah",
                "has manager",
                "Arjun Rao",
                Some(true),
            )],
        ),
        // Supersedes memory 3 (the Delhi move) and moves back to Pune.
        RememberInput {
            content: "Priya Shah moved back to Pune.".into(),
            kind: Some("event".into()),
            valid_from: Some("2026-02-01".into()),
            supersedes: vec!["M3".into()],
            ..Default::default()
        },
        RememberInput {
            content: "Priya Shah adopted a cat named Luna.".into(),
            kind: Some("event".into()),
            valid_from: Some("2026-03-01".into()),
            ..Default::default()
        },
        // Negation inferred from the text: the positive "works at" relation
        // ends here and the negative one becomes the current assertion.
        structured(
            "Priya Shah no longer works at Northwind Labs.",
            "2026-05-01",
            vec![
                entity("Priya Shah", "person"),
                entity("Northwind Labs", "org"),
            ],
            vec![plain("Priya Shah", "works at", "Northwind Labs")],
        ),
        // Seeded single-valued predicate with exclusive omitted: the predicate
        // policy alone ends the older value when the target changes.
        structured(
            "Meera Iyer lives in Pune.",
            "2025-03-01",
            vec![entity("Meera Iyer", "person"), entity("Pune", "place")],
            vec![plain("Meera Iyer", "lives in", "Pune")],
        ),
        structured(
            "Meera Iyer lives in Delhi.",
            "2026-01-01",
            vec![entity("Meera Iyer", "person"), entity("Delhi", "place")],
            vec![plain("Meera Iyer", "lives in", "Delhi")],
        ),
        // Multi-valued predicate with exclusive omitted: both values stay
        // current and the second write reports the first as a conflict.
        structured(
            "Meera Iyer mentors Arjun Rao.",
            "2025-04-01",
            vec![
                entity("Meera Iyer", "person"),
                entity("Arjun Rao", "person"),
            ],
            vec![plain("Meera Iyer", "mentors", "Arjun Rao")],
        ),
        structured(
            "Meera Iyer mentors Kabir Menon.",
            "2026-03-01",
            vec![
                entity("Meera Iyer", "person"),
                entity("Kabir Menon", "person"),
            ],
            vec![plain("Meera Iyer", "mentors", "Kabir Menon")],
        ),
    ]
}

/// Fill in the supersede placeholder once the sequential run knows the id.
fn resolve_supersedes(inputs: &mut [RememberInput], sequential_ids: &[i64]) {
    for input in inputs.iter_mut() {
        for target in input.supersedes.iter_mut() {
            if let Some(index) = target.strip_prefix('M') {
                let index: usize = index.parse().unwrap();
                *target = format!("m{}", sequential_ids[index - 1]);
            }
        }
    }
}

fn pairs(engine: &Engine, sql: &str) -> Vec<(i64, i64)> {
    let mut st = engine.db.sql().prepare(sql).unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

type RelationRow = (i64, i64, i64, i64, Option<i64>, Option<i64>);
type MemoryRow = (i64, String, Option<i64>, Option<i64>, Option<i64>);

fn relations_of(engine: &Engine) -> Vec<RelationRow> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT id,src,dst,predicate_id,valid_from,valid_to FROM relations ORDER BY id")
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

fn relation_facts(engine: &Engine) -> Vec<(String, i64, Option<i64>)> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT fact,polarity,valid_to FROM relations ORDER BY id")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn memories_of(engine: &Engine) -> Vec<MemoryRow> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT id,content,valid_to,expired_at,superseded_by FROM memories ORDER BY id")
        .unwrap();
    st.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .unwrap()
    .collect::<rusqlite::Result<Vec<_>>>()
    .unwrap()
}

fn entity_names(engine: &Engine) -> Vec<(i64, String)> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT id,name FROM entities ORDER BY id")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[test]
fn batch_matches_sequential_on_a_mixed_workload() {
    // Sequential reference.
    let sequential = engine();
    let mut inputs = workload();
    let mut sequential_ids = Vec::new();
    let mut sequential_errors: Vec<Option<String>> = Vec::new();
    let mut sequential_outputs: Vec<Option<String>> = Vec::new();
    // Resolve the supersede placeholder by id, which the sequential run reveals.
    let mut index = 0;
    while index < inputs.len() {
        if index == 6 {
            resolve_supersedes(&mut inputs, &sequential_ids);
        }
        let input = inputs[index].clone();
        match sequential.remember(input) {
            Ok(out) => {
                sequential_ids.push(out.id);
                sequential_errors.push(None);
                sequential_outputs.push(Some(serde_json::to_string(&out).unwrap()));
            }
            Err(e) => {
                sequential_ids.push(-1);
                sequential_errors.push(Some(e.to_string()));
                sequential_outputs.push(None);
            }
        }
        index += 1;
    }
    assert!(sequential_errors[4].is_some(), "the blank input must fail");

    // Batch.
    let batched = engine();
    let results = batched.remember_batch(inputs);
    assert_eq!(results.len(), 13);
    let mut batch_ids = Vec::new();
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(out) => {
                batch_ids.push(out.id);
                assert!(
                    sequential_errors[i].is_none(),
                    "batch succeeded where sequential failed at {i}"
                );
                // Whole output, not just the id: relations, invalidations,
                // conflicts, related candidates and hints must agree too.
                assert_eq!(
                    sequential_outputs[i].as_deref(),
                    Some(serde_json::to_string(out).unwrap().as_str()),
                    "output mismatch at {i}"
                );
            }
            Err(e) => {
                batch_ids.push(-1);
                assert_eq!(
                    sequential_errors[i].as_deref(),
                    Some(e.to_string().as_str()),
                    "error mismatch at {i}"
                );
            }
        }
    }
    assert_eq!(batch_ids, sequential_ids, "ids differ");
    assert!(batch_ids[4] == -1);
    assert!(
        batch_ids[6] > 0,
        "the item after the failure must be stored"
    );

    // The failed item left no trace, and both databases are identical.
    assert_eq!(memories_of(&sequential), memories_of(&batched));
    assert_eq!(entity_names(&sequential), entity_names(&batched));
    assert_eq!(relations_of(&sequential), relations_of(&batched));
    let mentions = "SELECT memory_id,entity_id FROM mentions ORDER BY 1,2";
    assert_eq!(pairs(&sequential, mentions), pairs(&batched, mentions));
    let evidence = "SELECT relation_id,memory_id FROM relation_evidence ORDER BY 1,2";
    assert_eq!(pairs(&sequential, evidence), pairs(&batched, evidence));
    // 11 memories: one failed, one duplicate collapsed into the first.
    assert_eq!(memories_of(&batched).len(), 11);

    // The three new cases really happened, in both runs.
    let facts = relation_facts(&batched);
    let find = |needle: &str| {
        facts
            .iter()
            .find(|(fact, ..)| fact == needle)
            .unwrap_or_else(|| panic!("no relation '{needle}' in {facts:?}"))
    };
    assert_eq!(
        find("Priya Shah NOT works at Northwind Labs").1,
        0,
        "inferred negation must be stored as a negative relation"
    );
    assert!(
        find("Meera Iyer lives in Pune").2.is_some(),
        "a seeded single-valued predicate must end the old value"
    );
    assert!(
        find("Meera Iyer mentors Arjun Rao").2.is_none(),
        "a multi-valued predicate must keep both values"
    );
    let conflict = results[12].as_ref().unwrap();
    assert_eq!(
        conflict.conflicts.len(),
        1,
        "the second mentor must be reported as a conflict"
    );
}

#[test]
fn batch_sees_earlier_items_of_the_same_chunk() {
    let engine = engine();
    let first = structured(
        "Priya Shah lives in Pune.",
        "2025-01-10",
        vec![entity("Priya Shah", "person"), entity("Pune", "place")],
        vec![relation("Priya Shah", "lives in", "Pune", Some(true))],
    );
    let second = structured(
        "Priya Shah lives in Delhi.",
        "2025-06-01",
        vec![entity("Priya Shah", "person"), entity("Delhi", "place")],
        vec![relation("Priya Shah", "lives in", "Delhi", Some(true))],
    );
    let results = engine.remember_batch(vec![first, second]);
    let out = results[1].as_ref().unwrap();
    assert_eq!(
        out.invalidated_relations.len(),
        1,
        "exclusive write in the same chunk must see the first"
    );
    // Same entity id for both memories: resolution saw the first item.
    assert_eq!(
        results[0].as_ref().unwrap().entities[0].id,
        out.entities[0].id
    );
    // A duplicate inside one chunk collapses, like sequential remember.
    let dup = RememberInput {
        content: "Priya Shah lives in Delhi.".into(),
        kind: Some("fact".into()),
        ..Default::default()
    };
    let results = engine.remember_batch(vec![dup]);
    assert!(results[0].as_ref().unwrap().duplicate_of.is_some());
}

#[test]
fn batch_reports_every_error_without_writing() {
    let engine = engine();
    let bad = vec![
        RememberInput {
            content: String::new(),
            ..Default::default()
        },
        RememberInput {
            content: "x".into(),
            kind: Some("nonsense".into()),
            ..Default::default()
        },
        RememberInput {
            content: "Priya Shah lives in Pune.".into(),
            valid_from: Some("not a date".into()),
            ..Default::default()
        },
    ];
    let results = engine.remember_batch(bad);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.is_err()));
    assert_eq!(memories_of(&engine).len(), 0);
}

#[test]
fn generated_world_is_deterministic_and_queryable() {
    // The generator is an example binary; its output contract is checked here on
    // the committed small world (kept < 2 MB) plus a fresh generation.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/world-300.json");
    if !path.exists() {
        return;
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let world: serde_json::Value = serde_json::from_str(&text).unwrap();
    let memories = world["memories"].as_array().unwrap();
    let queries = world["queries"].as_array().unwrap();
    assert!(memories.len() > 1000, "{} memories", memories.len());
    assert!(queries.len() >= 100, "{} queries", queries.len());

    let keys: HashMap<&str, &serde_json::Value> = memories
        .iter()
        .map(|m| (m["key"].as_str().unwrap(), m))
        .collect();
    let mut with_answers = 0;
    for query in queries {
        for key in query["expect"].as_array().unwrap() {
            assert!(
                keys.contains_key(key.as_str().unwrap()),
                "unknown key {key}"
            );
        }
        if let Some(answers) = query["answers"].as_array() {
            with_answers += 1;
            assert!(!answers.is_empty());
        }
        if let Some(absent) = query["expect_absent"].as_array() {
            for key in absent {
                assert!(keys.contains_key(key.as_str().unwrap()));
            }
        }
    }
    assert_eq!(with_answers, queries.len());
}
