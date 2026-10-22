//! Write-path regression and spec tests (design §5, §6, "Temporal rules", §7).

use neocortex::{
    embed::{Embedder, HashEmbedder, Mode},
    engine::{Engine, EntityIn, RelationIn, RememberInput, RememberOutput},
    kg,
    temporal::Clock,
    Result,
};
use std::collections::HashSet;
use std::sync::Arc;

const NOW: i64 = 1_700_000_000;
const T1: i64 = 1_700_000_000;
const T2: i64 = 1_700_086_400;

fn clock() -> Clock {
    Clock {
        now: NOW,
        tz_offset_secs: 0,
    }
}

fn engine_with(embedder: Arc<dyn Embedder>) -> Engine {
    Engine::open_in_memory(embedder)
        .unwrap()
        .with_clock(clock())
}

fn engine() -> Engine {
    engine_with(Arc::new(HashEmbedder))
}

fn axis(i: usize) -> Vec<f32> {
    let mut v = vec![0.0; 768];
    v[i] = 1.0;
    v
}

/// Unit vector at cosine `cos` from `axis(0)`.
fn at(cos: f32) -> Vec<f32> {
    let mut v = vec![0.0; 768];
    v[0] = cos;
    v[1] = (1.0 - cos * cos).sqrt();
    v
}

struct MapEmbedder {
    map: Vec<(String, Vec<f32>)>,
}

impl MapEmbedder {
    fn new(pairs: Vec<(&str, Vec<f32>)>) -> Self {
        MapEmbedder {
            map: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }
}

impl Embedder for MapEmbedder {
    fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>> {
        let fallback = HashEmbedder.embed(texts, mode)?;
        Ok(texts
            .iter()
            .zip(fallback)
            .map(|(text, fallback)| {
                self.map
                    .iter()
                    .find(|(key, _)| text.starts_with(key.as_str()))
                    .map(|(_, vector)| vector.clone())
                    .unwrap_or(fallback)
            })
            .collect())
    }

    fn name(&self) -> &str {
        "map"
    }
}

fn named(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        ..Default::default()
    }
}

fn rel(source: &str, predicate: &str, target: &str) -> RelationIn {
    RelationIn {
        source: source.into(),
        predicate: predicate.into(),
        target: target.into(),
        ..Default::default()
    }
}

fn remember(
    engine: &Engine,
    content: &str,
    entities: Vec<EntityIn>,
    relations: Vec<RelationIn>,
) -> RememberOutput {
    engine
        .remember(RememberInput {
            content: content.into(),
            entities,
            relations,
            ..Default::default()
        })
        .unwrap()
}

fn remember_at(
    engine: &Engine,
    content: &str,
    entities: Vec<EntityIn>,
    relations: Vec<RelationIn>,
    valid_from: i64,
) -> RememberOutput {
    engine
        .remember(RememberInput {
            content: content.into(),
            valid_from: Some(valid_from.to_string()),
            entities,
            relations,
            ..Default::default()
        })
        .unwrap()
}

fn entity_id(engine: &Engine, name: &str) -> i64 {
    engine
        .db
        .sql()
        .query_row(
            "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1",
            [neocortex::text::norm_name(name)],
            |r| r.get(0),
        )
        .unwrap()
}

fn count(engine: &Engine, sql: &str) -> i64 {
    engine.db.sql().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn relation_state(engine: &Engine, id: i64) -> (i64, Option<i64>, Option<i64>) {
    engine
        .db
        .sql()
        .query_row(
            "SELECT valid_from,valid_to,expired_at FROM relations WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

fn gen(engine: &Engine) -> i64 {
    engine
        .db
        .sql()
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='gen'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
}

// ---------------------------------------------------------------- resolution

#[test]
fn embedding_similarity_only_creates_similar_edges() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("London", axis(0)),
        ("New York City", at(0.9)),
    ])));
    let first = remember(
        &engine,
        "London is the capital",
        vec![named("London")],
        vec![],
    );
    let london = first.entities[0].id;
    let second = remember(
        &engine,
        "New York City is large",
        vec![named("New York City")],
        vec![],
    );
    assert_eq!(second.entities[0].status, "new");
    assert_ne!(second.entities[0].id, london);
    assert_eq!(second.entities[0].similar.len(), 1);
    assert_eq!(second.entities[0].similar[0].0, london);
    assert!((second.entities[0].similar[0].2 - 0.9).abs() < 1e-4);
    assert_eq!(count(&engine, "SELECT count(*) FROM similar"), 1);
    assert_eq!(count(&engine, "SELECT count(*) FROM entities"), 2);
}

#[test]
fn weak_lexical_guards_keep_entities_separate() {
    for (a, b) in [
        ("Paris TX", "Paris"),
        ("API v1", "API v2"),
        ("mother", "guardian"),
    ] {
        // Longer keys first so "Paris TX" is not shadowed by "Paris".
        let engine = engine_with(Arc::new(MapEmbedder::new(vec![(a, axis(0)), (b, axis(1))])));
        let first = remember(&engine, "first note", vec![named(a)], vec![]);
        let second = remember(&engine, "second note", vec![named(b)], vec![]);
        assert_eq!(second.entities[0].status, "new", "{a} vs {b}");
        assert_ne!(second.entities[0].id, first.entities[0].id, "{a} vs {b}");
        assert_eq!(
            count(&engine, "SELECT count(*) FROM entities"),
            2,
            "{a} vs {b}"
        );
    }
}

#[test]
fn acronym_and_lexical_merges() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("New York City", axis(0)),
        ("NYC", axis(0)),
    ])));
    let first = remember(&engine, "first note", vec![named("New York City")], vec![]);
    let second = remember(&engine, "second note", vec![named("NYC")], vec![]);
    assert_eq!(second.entities[0].status, "acronym");
    assert_eq!(second.entities[0].id, first.entities[0].id);

    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("PostgreSQL", axis(0)),
        ("Postgres", axis(1)),
    ])));
    let first = remember(&engine, "third note", vec![named("PostgreSQL")], vec![]);
    let second = remember(&engine, "fourth note", vec![named("Postgres")], vec![]);
    assert_eq!(second.entities[0].status, "lexical");
    assert_eq!(second.entities[0].id, first.entities[0].id);
    assert_eq!(count(&engine, "SELECT count(*) FROM entities"), 1);
}

#[test]
fn candidate_generation_finds_lexical_matches_outside_knn() {
    let mut pairs = vec![("PostgreSQL", axis(1)), ("Postgres", axis(0))];
    for (i, name) in [
        "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
        "Juliet", "Kilo", "Lima",
    ]
    .iter()
    .enumerate()
    {
        pairs.push((name, axis(i + 2)));
    }
    let engine = engine_with(Arc::new(MapEmbedder::new(pairs)));
    let first = remember(&engine, "database note", vec![named("PostgreSQL")], vec![]);
    for (i, name) in [
        "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
        "Juliet", "Kilo", "Lima",
    ]
    .iter()
    .enumerate()
    {
        remember(&engine, &format!("filler {i}"), vec![named(name)], vec![]);
    }
    let second = remember(
        &engine,
        "second database note",
        vec![named("Postgres")],
        vec![],
    );
    assert_eq!(second.entities[0].status, "lexical");
    assert_eq!(second.entities[0].id, first.entities[0].id);
}

#[test]
fn exact_matches_use_name_and_agent_aliases() {
    let engine = engine();
    let first = remember(
        &engine,
        "Mom lives in Delhi",
        vec![EntityIn {
            name: "Mom".into(),
            aliases: vec!["mother".into()],
            ..Default::default()
        }],
        vec![],
    );
    let mom = first.entities[0].id;
    let matched = remember(&engine, "Mom called", vec![named("Mom")], vec![]);
    assert_eq!(matched.entities[0].status, "matched");
    assert_eq!(matched.entities[0].id, mom);
    let aliased = remember(&engine, "Mother called", vec![named("Mother")], vec![]);
    assert_eq!(aliased.entities[0].status, "alias");
    assert_eq!(aliased.entities[0].id, mom);
}

#[test]
fn same_as_accepts_name_alias_and_rejects_unknown() {
    let engine = engine();
    let first = remember(
        &engine,
        "Mom lives in Delhi",
        vec![EntityIn {
            name: "Mom".into(),
            aliases: vec!["mother".into()],
            ..Default::default()
        }],
        vec![],
    );
    let mom = first.entities[0].id;
    for target in ["mother", &format!("e{mom}"), &mom.to_string()] {
        let out = engine
            .remember(RememberInput {
                content: format!("same_as {target}"),
                entities: vec![EntityIn {
                    name: "mum".into(),
                    same_as: Some(target.into()),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(out.entities[0].status, "same_as");
        assert_eq!(out.entities[0].id, mom);
    }
    let error = engine
        .remember(RememberInput {
            content: "unknown same_as".into(),
            entities: vec![EntityIn {
                name: "x".into(),
                same_as: Some("nobody".into()),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown entity"), "{error}");
}

#[test]
fn relation_endpoints_use_their_own_embedding() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("London", axis(0)),
        ("Paris", at(0.9)),
    ])));
    remember(
        &engine,
        "London move note",
        vec![named("London")],
        vec![rel("London", "lives in", "Paris")],
    );
    let london = entity_id(&engine, "London");
    let paris = entity_id(&engine, "Paris");
    assert_ne!(london, paris);
    let (a, b) = if london < paris {
        (london, paris)
    } else {
        (paris, london)
    };
    let score: f32 = engine
        .db
        .sql()
        .query_row(
            "SELECT score FROM similar WHERE a=?1 AND b=?2",
            [a, b],
            |r| r.get(0),
        )
        .unwrap();
    // vectors[1] (the London card) would give 1.0 here.
    assert!((score - 0.9).abs() < 1e-3, "score {score}");
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 1);
}

#[test]
fn mention_count_increments_once_per_mention_row() {
    let engine = engine();
    let first = remember(&engine, "Mom is here", vec![named("Mom")], vec![]);
    let mom = first.entities[0].id;
    let mention_count = |engine: &Engine| -> i64 {
        engine
            .db
            .sql()
            .query_row(
                "SELECT mention_count FROM entities WHERE id=?1",
                [mom],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(mention_count(&engine), 1);
    remember(&engine, "Mom is there", vec![named("Mom")], vec![]);
    assert_eq!(mention_count(&engine), 2);
    // A duplicate memory reuses the mention row.
    remember(&engine, "Mom is here", vec![named("Mom")], vec![]);
    assert_eq!(mention_count(&engine), 2);
    engine.add_alias("Mom", "mother").unwrap();
    assert_eq!(mention_count(&engine), 2);
}

#[test]
fn gazetteer_links_known_aliases_in_content() {
    let engine = engine();
    let first = remember(
        &engine,
        "Mom is here",
        vec![EntityIn {
            name: "Mom".into(),
            aliases: vec!["mother".into()],
            ..Default::default()
        }],
        vec![],
    );
    let mom = first.entities[0].id;
    let second = remember(&engine, "the mother called", vec![], vec![]);
    let mentioned: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT count(*) FROM mentions WHERE memory_id=?1 AND entity_id=?2",
            [second.id, mom],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mentioned, 1);
    let edges: i64 = engine
        .db
        .graph()
        .cypher("MATCH (:Memory {id:'m:2'})-[:MENTIONS]->(:Entity {id:'e:1'}) RETURN count(*) AS c")
        .unwrap()
        .iter()
        .next()
        .and_then(|r| r.get::<i64>("c").ok())
        .unwrap_or(0);
    assert_eq!(edges, 1);
}

#[test]
fn entity_enrichment_fills_and_appends_description() {
    let engine = engine();
    let first = remember(&engine, "Acme is a supplier", vec![named("Acme")], vec![]);
    let acme = first.entities[0].id;
    remember(
        &engine,
        "Acme hired us",
        vec![EntityIn {
            name: "Acme".into(),
            kind: Some("company".into()),
            description: Some("a supplier".into()),
            ..Default::default()
        }],
        vec![],
    );
    let (kind, description): (String, String) = engine
        .db
        .sql()
        .query_row(
            "SELECT type,description FROM entities WHERE id=?1",
            [acme],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, "company");
    assert_eq!(description, "a supplier");
    remember(
        &engine,
        "Acme expanded",
        vec![EntityIn {
            name: "Acme".into(),
            description: Some("grew in 2026".into()),
            ..Default::default()
        }],
        vec![],
    );
    let description: String = engine
        .db
        .sql()
        .query_row(
            "SELECT description FROM entities WHERE id=?1",
            [acme],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(description, "a supplier; grew in 2026");
    engine.add_alias("Acme", "The Acme Corp.").unwrap();
    let aliases: String = engine
        .db
        .sql()
        .query_row(
            "SELECT aliases FROM entities_fts WHERE rowid=?1",
            [acme],
            |r| r.get(0),
        )
        .unwrap();
    assert!(aliases.contains("acme corp"), "{aliases}");
}

#[test]
fn predicate_knn_creates_alias_and_or_merges_exclusive() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("Alice lives in Paris", axis(0)),
        ("Alice resides in Paris", at(0.95)),
    ])));
    let first = remember(
        &engine,
        "Alice lives in Paris note",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Paris")],
    );
    let second = remember(
        &engine,
        "Alice resides in Paris note",
        vec![named("Alice")],
        vec![RelationIn {
            exclusive: true,
            ..rel("Alice", "resides in", "Paris")
        }],
    );
    let p1: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT predicate_id FROM relations WHERE id=?1",
            [first.relations[0].id],
            |r| r.get(0),
        )
        .unwrap();
    let p2: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT predicate_id FROM relations WHERE id=?1",
            [second.relations[0].id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(p1, p2);
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM predicate_aliases WHERE alias_norm='reside in' AND predicate_id=1"
        ),
        1
    );
    let exclusive: i64 = engine
        .db
        .sql()
        .query_row("SELECT exclusive FROM predicates WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(exclusive, 1);
}

// ------------------------------------------------------------------ relations

#[test]
fn exclusive_older_relation_ends_at_new_start() {
    let engine = engine();
    let first = remember_at(
        &engine,
        "Delhi home",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Delhi")],
        T1,
    );
    let delhi = first.relations[0].id;
    let second = remember_at(
        &engine,
        "Pune home",
        vec![named("Alice")],
        vec![RelationIn {
            exclusive: true,
            ..rel("Alice", "lives in", "Pune")
        }],
        T2,
    );
    assert_eq!(relation_state(&engine, delhi), (T1, Some(T2), None));
    assert!(second.invalidated_relations.contains(&delhi));
    assert_eq!(relation_state(&engine, second.relations[0].id).1, None);
}

#[test]
fn exclusive_backfill_is_stored_already_ended() {
    let engine = engine();
    let later = remember_at(
        &engine,
        "Pune home",
        vec![named("Alice")],
        vec![RelationIn {
            exclusive: true,
            ..rel("Alice", "lives in", "Pune")
        }],
        T2,
    );
    let pune = later.relations[0].id;
    let backfill = remember_at(
        &engine,
        "Delhi home",
        vec![named("Alice")],
        vec![RelationIn {
            exclusive: true,
            ..rel("Alice", "lives in", "Delhi")
        }],
        T1,
    );
    let delhi = backfill.relations[0].id;
    assert_eq!(relation_state(&engine, delhi), (T1, Some(T2), None));
    assert_eq!(relation_state(&engine, pune), (T2, None, None));
}

#[test]
fn exclusive_equal_start_retires_the_old_relation() {
    let engine = engine();
    let first = remember_at(
        &engine,
        "Delhi home",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Delhi")],
        T1,
    );
    let delhi = first.relations[0].id;
    remember_at(
        &engine,
        "Pune home",
        vec![named("Alice")],
        vec![RelationIn {
            exclusive: true,
            ..rel("Alice", "lives in", "Pune")
        }],
        T1,
    );
    assert_eq!(relation_state(&engine, delhi), (T1, Some(T1), Some(NOW)));
}

#[test]
fn non_exclusive_predicate_invalidates_nothing() {
    let engine = engine();
    let first = remember_at(
        &engine,
        "Delhi home",
        vec![named("Alice")],
        vec![rel("Alice", "likes", "Delhi")],
        T1,
    );
    remember_at(
        &engine,
        "Pune home",
        vec![named("Alice")],
        vec![rel("Alice", "likes", "Pune")],
        T2,
    );
    assert_eq!(
        relation_state(&engine, first.relations[0].id),
        (T1, None, None)
    );
}

#[test]
fn relation_upsert_only_matches_currently_valid_rows() {
    let engine = engine();
    let first = remember_at(
        &engine,
        "Delhi home",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Delhi")],
        T1,
    );
    let original = first.relations[0].id;
    engine
        .invalidate(&format!("r{original}"), Some(&T2.to_string()))
        .unwrap();
    let again = remember_at(
        &engine,
        "Delhi home again",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Delhi")],
        T2,
    );
    assert_eq!(again.relations[0].status, "new");
    assert_ne!(again.relations[0].id, original);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 2);
    let fact: String = engine
        .db
        .sql()
        .query_row(
            "SELECT fact FROM relations_fts WHERE rowid=?1",
            [again.relations[0].id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fact, "Alice lives in Delhi");
}

// --------------------------------------------------------------- generation

#[test]
fn every_write_bumps_the_generation() {
    let engine = engine();
    remember(&engine, "first", vec![named("A"), named("B")], vec![]);
    assert_eq!(gen(&engine), 1);
    engine
        .invalidate("m1", Some(&(T1 + 10).to_string()))
        .unwrap();
    assert_eq!(gen(&engine), 2);
    engine.forget("m1", false).unwrap();
    assert_eq!(gen(&engine), 3);
    engine.add_alias("A", "alpha").unwrap();
    assert_eq!(gen(&engine), 4);
    engine.merge_entities("A", "B").unwrap();
    assert_eq!(gen(&engine), 5);
    engine.reindex().unwrap();
    assert_eq!(gen(&engine), 6);
}

// ------------------------------------------------------------------ forgetting

#[test]
fn forget_soft_keeps_history_in_the_projection() {
    let engine = engine();
    let first = remember(&engine, "keep me", vec![named("A")], vec![]);
    engine.forget(&format!("m{}", first.id), false).unwrap();
    let expired: Option<i64> = engine
        .db
        .sql()
        .query_row(
            "SELECT expired_at FROM memories WHERE id=?1",
            [first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(expired, Some(NOW));
    let nodes: i64 = engine
        .db
        .graph()
        .cypher("MATCH (n:Memory {id:'m:1'}) RETURN count(n) AS c")
        .unwrap()
        .iter()
        .next()
        .and_then(|r| r.get::<i64>("c").ok())
        .unwrap_or(0);
    assert_eq!(nodes, 1);
    let problems = engine.validate().unwrap();
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn forget_hard_removes_orphaned_relations_and_validates() {
    let engine = engine();
    let first = remember(
        &engine,
        "Alice lives in Delhi",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Delhi")],
    );
    engine.forget(&format!("m{}", first.id), true).unwrap();
    assert_eq!(count(&engine, "SELECT count(*) FROM memories"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM mentions"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relation_evidence"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_fts"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_vec"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM memories_vec"), 0);
    let nodes: i64 = engine
        .db
        .graph()
        .cypher("MATCH (n:Memory) RETURN count(n) AS c")
        .unwrap()
        .iter()
        .next()
        .and_then(|r| r.get::<i64>("c").ok())
        .unwrap_or(0);
    assert_eq!(nodes, 0);
    assert!(engine.validate().unwrap().is_empty());
}

// -------------------------------------------------------------------- snapshot

#[test]
fn snapshot_excludes_forgotten_and_keeps_superseded_when_asked() {
    let engine = engine();
    let m1 = remember(&engine, "forgotten note", vec![named("A")], vec![]);
    let m2 = remember(&engine, "old note", vec![named("B")], vec![]);
    engine.forget(&format!("m{}", m1.id), false).unwrap();
    let m3 = engine
        .remember(RememberInput {
            content: "new note".into(),
            entities: vec![named("B")],
            supersedes: vec![format!("m{}", m2.id)],
            ..Default::default()
        })
        .unwrap();
    let live = kg::snapshot(&engine.db, None, false).unwrap();
    assert!(!live.mem_node.contains_key(&m1.id));
    assert!(!live.mem_node.contains_key(&m2.id));
    assert!(live.mem_node.contains_key(&m3.id));
    let ended = kg::snapshot(&engine.db, None, true).unwrap();
    assert!(!ended.mem_node.contains_key(&m1.id));
    assert!(ended.mem_node.contains_key(&m2.id));
    assert!(ended.mem_node.contains_key(&m3.id));
}

#[test]
fn snapshot_relation_weight_uses_log_evidence() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("A", axis(0)),
        ("B", axis(1)),
    ])));
    let first = remember(
        &engine,
        "weighted relation",
        vec![named("A")],
        vec![rel("A", "knows", "B")],
    );
    engine
        .db
        .sql()
        .execute(
            "UPDATE relations SET evidence_count=10 WHERE id=?1",
            [first.relations[0].id],
        )
        .unwrap();
    let snapshot = kg::snapshot(&engine.db, None, true).unwrap();
    let a = snapshot.ent_node[&entity_id(&engine, "A")] as usize;
    let b = snapshot.ent_node[&entity_id(&engine, "B")] as usize;
    let mut found = None;
    for index in snapshot.csr.offsets[a]..snapshot.csr.offsets[a + 1] {
        if snapshot.csr.targets[index] as usize == b {
            found = Some(snapshot.csr.weights[index]);
        }
    }
    let weight = found.expect("edge A-B");
    assert!((weight - 11f32.ln()).abs() < 1e-4, "weight {weight}");
}

// ----------------------------------------------------------------- communities

#[test]
fn communities_persist_two_clusters() {
    let engine = engine();
    engine
        .db
        .write(|db| {
            for (id, name) in [(1, "Alpha"), (2, "Bravo"), (3, "Charlie"), (4, "Delta")] {
                db.sql().execute(
                    "INSERT INTO entities(id,name,norm,type,description,created_at,updated_at,mention_count)
                     VALUES (?1,?2,?3,'','',0,0,0)",
                    rusqlite::params![id, name, neocortex::text::norm_name(name)],
                )?;
                kg::project_entity(db, id, name, "")?;
            }
            kg::project_similar(db, 1, 2, 1.0)?;
            kg::project_similar(db, 3, 4, 1.0)?;
            Ok(())
        })
        .unwrap();
    let result = kg::communities(&engine.db).unwrap();
    assert_eq!(result.len(), 4);
    let distinct: HashSet<i64> = result.values().copied().collect();
    assert_eq!(distinct.len(), 2, "{result:?}");
    assert_eq!(count(&engine, "SELECT count(*) FROM communities"), 4);
    let stamp: i64 = engine
        .db
        .sql()
        .query_row("SELECT min(computed_at) FROM communities", [], |r| r.get(0))
        .unwrap();
    assert!(stamp > 0);
    let stored: String = engine
        .db
        .sql()
        .query_row(
            "SELECT value FROM meta WHERE key='communities_gen'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, "0");
}

// --------------------------------------------------------------- merge/alias

#[test]
fn merge_entities_moves_edges_and_cleans_up() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("A", axis(0)),
        ("B", axis(1)),
        ("C", axis(2)),
    ])));
    let first = remember(
        &engine,
        "A knows B",
        vec![named("A"), named("B")],
        vec![rel("A", "knows", "B")],
    );
    let a = first.entities[0].id;
    let b = first.entities[1].id;
    let third = remember(&engine, "C is here", vec![named("C")], vec![]);
    let c = third.entities[0].id;
    engine
        .db
        .write(|db| {
            db.sql().execute(
                "INSERT OR REPLACE INTO similar(a,b,score) VALUES (?1,?2,0.8)",
                rusqlite::params![a, b],
            )?;
            db.sql().execute(
                "INSERT OR REPLACE INTO similar(a,b,score) VALUES (?1,?2,0.9)",
                rusqlite::params![b.min(c), b.max(c)],
            )?;
            kg::project_similar(db, a, b, 0.8)?;
            kg::project_similar(db, b.min(c), b.max(c), 0.9)?;
            Ok(())
        })
        .unwrap();
    engine
        .merge_entities(&format!("e{a}"), &format!("e{b}"))
        .unwrap();
    assert_eq!(count(&engine, "SELECT count(*) FROM entities"), 2);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_fts"), 0);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_vec"), 0);
    let similar: Vec<(i64, i64, f32)> = {
        let mut st = engine
            .db
            .sql()
            .prepare("SELECT a,b,score FROM similar ORDER BY a,b")
            .unwrap();
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    let (lo, hi) = if a < c { (a, c) } else { (c, a) };
    assert_eq!(similar, vec![(lo, hi, 0.9)]);
    let mention_count: i64 = engine
        .db
        .sql()
        .query_row("SELECT mention_count FROM entities WHERE id=?1", [a], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(mention_count, 2);
    assert_eq!(
        count(
            &engine,
            &format!("SELECT count(*) FROM entities_fts WHERE rowid={b}")
        ),
        0
    );
    let aliases: String = engine
        .db
        .sql()
        .query_row(
            "SELECT aliases FROM entities_fts WHERE rowid=?1",
            [a],
            |r| r.get(0),
        )
        .unwrap();
    assert!(aliases.contains('b'), "{aliases}");
    assert!(engine.validate().unwrap().is_empty());
}

#[test]
fn add_alias_normalises_and_rejects_conflicts() {
    let engine = engine();
    remember(&engine, "Acme note", vec![named("Acme")], vec![]);
    remember(&engine, "Beta note", vec![named("Beta")], vec![]);
    let acme = entity_id(&engine, "Acme");
    engine.add_alias("Acme", "The Acme Corp.").unwrap();
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM entity_aliases WHERE alias_norm='acme corp' AND entity_id=1"
        ),
        1
    );
    let aliases: String = engine
        .db
        .sql()
        .query_row(
            "SELECT aliases FROM entities_fts WHERE rowid=?1",
            [acme],
            |r| r.get(0),
        )
        .unwrap();
    assert!(aliases.contains("acme corp"), "{aliases}");
    let error = engine
        .add_alias("Beta", "acme corp")
        .unwrap_err()
        .to_string();
    assert!(error.contains("merge entities first"), "{error}");
}

// ------------------------------------------------------------------- validate

#[test]
fn validate_is_clean_after_a_mixed_workload_and_catches_drift() {
    let engine = engine();
    let first = remember(
        &engine,
        "A knows B",
        vec![named("A"), named("B")],
        vec![rel("A", "knows", "B")],
    );
    engine
        .invalidate(
            &format!("r{}", first.relations[0].id),
            Some(&T2.to_string()),
        )
        .unwrap();
    engine.forget(&format!("m{}", first.id), false).unwrap();
    engine.add_alias("A", "alpha").unwrap();
    remember(&engine, "C is here", vec![named("C")], vec![]);
    engine.reindex().unwrap();
    assert!(engine.validate().unwrap().is_empty());
    engine
        .db
        .graph()
        .cypher("MATCH (n:Memory {id:'m:1'}) DETACH DELETE n")
        .unwrap();
    let problems = engine.validate().unwrap();
    assert!(!problems.is_empty(), "expected projection drift");
}

// -------------------------------------------------------------- spec coverage

#[test]
fn duplicate_returns_existing_id_and_kind_validation_is_explicit() {
    let engine = engine();
    let input = RememberInput {
        content: "same durable fact".into(),
        ..Default::default()
    };
    let first = engine.remember(input.clone()).unwrap();
    let second = engine.remember(input).unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.duplicate_of, Some(first.id));
    let error = engine
        .remember(RememberInput {
            content: "bad".into(),
            kind: Some("unsupported".into()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("fact") && error.contains("note"));
}

#[test]
fn near_duplicate_is_reported_but_stored() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("alpha beta", axis(0)),
        ("delta epsilon", at(0.98)),
    ])));
    let first = remember(&engine, "alpha beta gamma", vec![], vec![]);
    let second = remember(&engine, "delta epsilon zeta", vec![], vec![]);
    assert_ne!(second.id, first.id);
    let (id, score) = second.near_duplicate.expect("near duplicate");
    assert_eq!(id, first.id);
    assert!(score >= 0.97, "{score}");
}

#[test]
fn relative_valid_from_is_parsed_against_the_clock() {
    let engine = engine();
    let out = engine
        .remember(RememberInput {
            content: "yesterday note".into(),
            valid_from: Some("yesterday".into()),
            ..Default::default()
        })
        .unwrap();
    let expected = neocortex::temporal::parse_instant("yesterday", &clock()).unwrap();
    let stored: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT valid_from FROM memories WHERE id=?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, expected);
    assert!(stored < NOW);
}

#[test]
fn heuristic_entities_are_used_when_none_are_supplied() {
    let engine = engine();
    let out = remember(
        &engine,
        "Deployed the service to Paris on Monday.",
        vec![],
        vec![],
    );
    assert!(
        out.entities
            .iter()
            .any(|e| neocortex::text::norm_name(&e.name) == "paris"),
        "{:?}",
        out.entities
    );
    assert!(entity_id(&engine, "Paris") > 0);
}

#[test]
fn supersedes_retires_the_old_memory() {
    let engine = engine();
    let first = remember(&engine, "old fact", vec![], vec![]);
    let second = engine
        .remember(RememberInput {
            content: "new fact".into(),
            supersedes: vec![format!("m{}", first.id)],
            ..Default::default()
        })
        .unwrap();
    let (expired, superseded_by, valid_to): (Option<i64>, Option<i64>, Option<i64>) = engine
        .db
        .sql()
        .query_row(
            "SELECT expired_at,superseded_by,valid_to FROM memories WHERE id=?1",
            [first.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(expired, Some(NOW));
    assert_eq!(superseded_by, Some(second.id));
    assert!(valid_to.is_some());
    let live: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT live FROM memories_vec WHERE rowid=?1",
            [first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(live, 0);
    assert_eq!(second.superseded_memories, vec![first.id]);
}

#[test]
fn invalidate_sets_valid_to_and_rejects_bad_input() {
    let engine = engine();
    let first = remember(&engine, "fact to end", vec![], vec![]);
    engine
        .invalidate(&format!("m{}", first.id), Some(&T2.to_string()))
        .unwrap();
    let valid_to: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT valid_to FROM memories WHERE id=?1",
            [first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(valid_to, T2);
    assert!(engine
        .invalidate(&format!("m{}", first.id), Some(&(T1 - 1).to_string()))
        .unwrap_err()
        .to_string()
        .contains("precedes"));
    assert!(engine
        .invalidate("m999", None)
        .unwrap_err()
        .to_string()
        .contains("unknown memory"));
}

#[test]
fn cypher_rejects_writes_and_allows_reads() {
    let engine = engine();
    remember(&engine, "some note", vec![named("A")], vec![]);
    for query in [
        "CREATE (:Entity {id:'e:99'})",
        "MATCH (n) DETACH DELETE n",
        "MATCH (n) SET n.name='x'",
    ] {
        let error = engine.cypher(query).unwrap_err().to_string();
        assert!(error.contains("read-only"), "{error}");
    }
    let result = engine.cypher("MATCH (n) RETURN count(n) AS c").unwrap();
    assert_eq!(result[0]["c"], 2);
}

#[test]
fn embedder_identity_rejects_a_different_name() {
    struct Other;
    impl Embedder for Other {
        fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>> {
            HashEmbedder.embed(texts, mode)
        }
        fn name(&self) -> &str {
            "other"
        }
    }
    let engine = engine();
    remember(&engine, "identity", vec![], vec![]);
    let other = Engine {
        db: engine.db,
        emb: Arc::new(Other),
        cfg: Default::default(),
        fixed_clock: Some(clock()),
        snapshot_cache: std::sync::Mutex::new(None),
    };
    assert!(other
        .remember(RememberInput {
            content: "second".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string()
        .contains("built with embedder 'hash'"));
}

#[test]
fn stats_counts_the_workload() {
    let engine = engine();
    remember(
        &engine,
        "A knows B",
        vec![named("A"), named("B")],
        vec![rel("A", "knows", "B")],
    );
    let stats = engine.stats().unwrap();
    assert_eq!(stats.memories, 1);
    assert_eq!(stats.live_memories, 1);
    assert_eq!(stats.entities, 2);
    assert_eq!(stats.relations, 1);
    assert_eq!(stats.live_relations, 1);
    assert_eq!(stats.predicates, 1);
    assert_eq!(stats.embedder, "hash");
}

#[test]
fn memory_ends_when_all_its_facts_end() {
    let engine = engine();
    let rel = |target: &str| neocortex::engine::RelationIn {
        source: "Alex".into(),
        predicate: "lives in".into(),
        target: target.into(),
        exclusive: true,
        ..Default::default()
    };
    let old = engine
        .remember(RememberInput {
            content: "Alex lives in Austin.".into(),
            valid_from: Some("2025-11-15".into()),
            relations: vec![rel("Austin")],
            ..Default::default()
        })
        .unwrap()
        .id;
    let mixed = engine
        .remember(RememberInput {
            content: "Alex lives in Austin and works at Helio.".into(),
            valid_from: Some("2025-11-16".into()),
            relations: vec![
                rel("Austin"),
                neocortex::engine::RelationIn {
                    source: "Alex".into(),
                    predicate: "works at".into(),
                    target: "Helio".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
        .unwrap()
        .id;
    engine
        .remember(RememberInput {
            content: "Alex moved to Denver.".into(),
            valid_from: Some("2026-07-01".into()),
            relations: vec![rel("Denver")],
            ..Default::default()
        })
        .unwrap();
    let valid_to = |id: i64| -> Option<i64> {
        engine
            .db
            .sql()
            .query_row("SELECT valid_to FROM memories WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let denver = neocortex::temporal::parse_instant("2026-07-01", &clock()).unwrap();
    assert_eq!(valid_to(old), Some(denver));
    assert_eq!(valid_to(mixed), None, "still evidences a live fact");
}
