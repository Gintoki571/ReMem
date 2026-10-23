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
        // Both phrases of the paraphrase pair share one vector, so the KNN
        // match is decided by the head/tail guard, not by cosine noise.
        ("lives in", at(0.99)),
        ("dwells in", at(0.99)),
    ])));
    let first = remember(
        &engine,
        "Alice lives in Paris note",
        vec![named("Alice")],
        vec![rel("Alice", "lives in", "Paris")],
    );
    let second = remember(
        &engine,
        "Alice dwells in Paris note",
        vec![named("Alice")],
        vec![rel("Alice", "dwells in", "Paris")],
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
    // The phrase vector of "dwells in" matched "lives in" (cosine 1.0, same
    // tail), so a second write lands on the same predicate group: same
    // subject+object+group is one fact, evidence accumulates on one row.
    assert_eq!(p2, p1);
    assert_eq!(second.relations[0].id, first.relations[0].id);
    let (evidence, group): (i64, i64) = engine
        .db
        .sql()
        .query_row(
            "SELECT evidence_count, group_id FROM relations r JOIN predicates p ON p.id=r.predicate_id
             WHERE r.id=?1",
            [first.relations[0].id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(evidence, 2);
    assert_eq!(group, 1, "live in is the head of its own group");
    // The exclusive flag sticks to the canonical predicate.
    let exclusive: i64 = engine
        .db
        .sql()
        .query_row("SELECT exclusive FROM predicates WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(exclusive, 1);
    // The agent's original phrase is kept on the newest relation.
    let text: String = engine
        .db
        .sql()
        .query_row("SELECT predicate_text FROM relations WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(text, "lives in");
}

#[test]
fn resides_in_is_grouped_with_lives_in_for_exclusivity() {
    // rel.md ER-01: "resides in" and "lives in" must fight over the subject,
    // otherwise two home facts stay current.
    let engine = engine();
    let first = remember(
        &engine,
        "Alice lives in Austin.",
        vec![named("Alice"), named("Austin")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alice", "lives in", "Austin")
        }],
    );
    let second = remember(
        &engine,
        "Alice resides in Denver as of",
        vec![named("Alice"), named("Denver")],
        vec![rel("Alice", "resides in", "Denver")],
    );
    assert_eq!(second.invalidated_relations, vec![first.relations[0].id]);
    let card = engine.entity("Alice", 20).unwrap();
    assert_eq!(card.current_facts.len(), 1);
    assert_eq!(card.current_facts[0].target, "Denver");
    // The predicate rows stay separate; the group is shared.
    let groups: Vec<String> = engine
        .db
        .sql()
        .prepare(
            "SELECT (SELECT canonical FROM predicates g WHERE g.id=p.group_id)
             FROM predicates p WHERE p.canonical IN ('live in','reside in') ORDER BY p.id",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap();
    assert_eq!(groups, vec!["live in".to_string(), "live in".to_string()]);
}

#[test]
fn role_nouns_and_tail_changes_never_alias() {
    // rel.md ER-02: "CEO of" and "CTO of" are different facts even though the
    // phrases embed close; rel.md ER-09: "lives near" is not "lives in".
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        ("Alice is CEO of", at(0.99)),
        ("Alice is CTO of", at(0.99)),
        ("Alice lives near", at(0.99)),
    ])));
    let ceo = remember(
        &engine,
        "Alice is CEO of Acme.",
        vec![named("Alice"), named("Acme")],
        vec![rel("Alice", "CEO of", "Acme")],
    );
    let cto = remember(
        &engine,
        "Alice is CTO of Acme.",
        vec![named("Alice"), named("Acme")],
        vec![rel("Alice", "CTO of", "Acme")],
    );
    assert_ne!(cto.relations[0].id, ceo.relations[0].id);
    assert_eq!(cto.invalidated_relations, Vec::<i64>::new());
    let card = engine.entity("Alice", 20).unwrap();
    assert_eq!(card.current_facts.len(), 2);
    let near = remember(
        &engine,
        "Alice lives near Austin.",
        vec![named("Alice"), named("Austin")],
        vec![rel("Alice", "lives near", "Austin")],
    );
    let (near_predicate, live_predicate): (i64, i64) = engine
        .db
        .sql()
        .query_row(
            "SELECT r.predicate_id, (SELECT id FROM predicates WHERE canonical='live in')
             FROM relations r WHERE r.id=?1",
            [near.relations[0].id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_ne!(near_predicate, live_predicate);
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
            exclusive: Some(true),
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
            exclusive: Some(true),
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
            exclusive: Some(true),
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
            exclusive: Some(true),
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
    assert_eq!(again.relations[0].status, "merged");
    assert_eq!(again.relations[0].id, original);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 1);
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
    engine.merge_entities("A", "B", false).unwrap();
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
        .merge_entities(&format!("e{a}"), &format!("e{b}"), false)
        .unwrap();
    assert_eq!(count(&engine, "SELECT count(*) FROM entities"), 2);
    // Rel ER-06: the relation that became a self-loop (A→B with B merged into
    // A) is kept as an expired row, never silently deleted.
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 1);
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM relations WHERE expired_at IS NULL"
        ),
        0
    );
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_fts"), 1);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations_vec"), 1);
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
    assert!(stats.predicates >= 12);
    assert_eq!(stats.embedder, "hash");
}

#[test]
fn memory_ends_when_all_its_facts_end() {
    let engine = engine();
    let rel = |target: &str| neocortex::engine::RelationIn {
        source: "Alex".into(),
        predicate: "lives in".into(),
        target: target.into(),
        exclusive: Some(true),
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

// ------------------------------------------------- audit regressions (#2..#22)

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "neocortex-writepath-{}-{tag}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine_at(path: &std::path::Path) -> Engine {
    Engine::open(path, Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(clock())
}

/// #2: hard forget must touch only the forgotten node/edges. A full rebuild of
/// a 2k-memory projection takes seconds; the bound is deliberately generous.
#[test]
fn hard_forget_of_one_memory_does_not_rebuild_the_projection() {
    let engine = engine();
    let mut ids = Vec::new();
    for i in 0..2000 {
        ids.push(
            engine
                .remember(RememberInput {
                    content: format!("bulk memory {i} about the staging host"),
                    entities: vec![named("Bulk")],
                    ..Default::default()
                })
                .unwrap()
                .id,
        );
    }
    assert_eq!(count(&engine, "SELECT count(*) FROM memories"), 2000);
    let victim = ids[1000];
    let started = std::time::Instant::now();
    engine.forget(&format!("m{victim}"), true).unwrap();
    let elapsed = started.elapsed();
    println!("hard forget of one memory in a 2k DB: {elapsed:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "hard forget took {elapsed:?}; the projection was probably rebuilt"
    );
    assert_eq!(count(&engine, "SELECT count(*) FROM memories"), 1999);
    let nodes: i64 = engine
        .db
        .graph()
        .cypher("MATCH (n:Memory) RETURN count(n) AS c")
        .unwrap()
        .iter()
        .next()
        .and_then(|r| r.get::<i64>("c").ok())
        .unwrap_or(0);
    assert_eq!(nodes, 1999);
    assert!(engine.validate().unwrap().is_empty());
}

/// #2: merging entities must leave the projection exact without a rebuild.
#[test]
fn merge_entities_reprojects_only_the_surviving_entity() {
    let engine = engine();
    let first = remember(
        &engine,
        "Acme makes widgets",
        vec![named("Acme"), named("Widget")],
        vec![rel("Acme", "makes", "Widget")],
    );
    let second = remember(&engine, "Beta is a partner", vec![named("Beta")], vec![]);
    assert!(engine.validate().unwrap().is_empty());
    engine
        .merge_entities(
            &format!("e{}", first.entities[0].id),
            &format!("e{}", second.entities[0].id),
            false,
        )
        .unwrap();
    assert_eq!(count(&engine, "SELECT count(*) FROM entities"), 2);
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 1);
    let problems = engine.validate().unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let facts = engine.entity("Acme", 10).unwrap().current_facts;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].target, "Widget");
}

/// #4: a soft forget ends relations whose only live evidence was that memory.
#[test]
fn soft_forget_ends_relations_with_no_other_live_evidence() {
    let engine = engine();
    let first = remember(
        &engine,
        "Mom lives in Delhi",
        vec![named("Mom")],
        vec![rel("Mom", "lives in", "Delhi")],
    );
    let rid = first.relations[0].id;
    engine.forget(&format!("m{}", first.id), false).unwrap();
    let (_, valid_to, expired_at) = relation_state(&engine, rid);
    // Retracted, not ended: forgetting the source is not a world-time event.
    assert_eq!(valid_to, None, "forget must not invent an end date");
    assert_eq!(expired_at, Some(NOW));
    let current = engine.entity("Mom", 10).unwrap().current_facts;
    assert!(
        current.is_empty(),
        "forgotten fact still current: {current:?}"
    );
}

/// #4: evidence from another live memory keeps the relation current.
#[test]
fn soft_forget_keeps_relations_with_other_live_evidence() {
    let engine = engine();
    let first = remember(
        &engine,
        "Mom lives in Delhi",
        vec![named("Mom")],
        vec![rel("Mom", "lives in", "Delhi")],
    );
    let rid = first.relations[0].id;
    let second = remember(
        &engine,
        "Mom still lives in Delhi",
        vec![named("Mom")],
        vec![rel("Mom", "lives in", "Delhi")],
    );
    assert_eq!(second.relations[0].id, rid, "the relation is shared");
    engine.forget(&format!("m{}", first.id), false).unwrap();
    let (_, valid_to, expired_at) = relation_state(&engine, rid);
    assert_eq!(valid_to, None);
    assert_eq!(expired_at, None);
}

/// #4: supersede ends the relation at the new memory's world-time start and
/// leaves it true-in-the-past (expired_at stays NULL).
#[test]
fn supersede_ends_relations_at_the_new_valid_from() {
    let engine = engine();
    let old = remember_at(
        &engine,
        "Mom lives in Delhi",
        vec![named("Mom")],
        vec![rel("Mom", "lives in", "Delhi")],
        NOW - 86_400,
    );
    let rid = old.relations[0].id;
    engine
        .remember(RememberInput {
            content: "Mom lives in Pune".into(),
            entities: vec![named("Mom")],
            relations: vec![rel("Mom", "lives in", "Pune")],
            supersedes: vec![format!("m{}", old.id)],
            ..Default::default()
        })
        .unwrap();
    let (_, valid_to, expired_at) = relation_state(&engine, rid);
    assert_eq!(valid_to, Some(NOW), "ends where the new memory starts");
    assert_eq!(expired_at, None, "it was true until then, not wrong");
    let current: Vec<i64> = engine
        .entity("Mom", 10)
        .unwrap()
        .current_facts
        .iter()
        .map(|f| f.id)
        .collect();
    assert!(!current.contains(&rid), "{current:?}");
}

/// #9: merging two entities that described the same fact leaves one relation,
/// and an exclusive predicate keeps only the newest object.
#[test]
fn merge_entities_dedupes_relations_and_reapplies_exclusivity() {
    let engine = engine_with(Arc::new(MapEmbedder::new(vec![
        // Fact sentences first: the embedder matches on prefix, and predicate
        // vectors must not look alike (that would alias the predicates).
        ("Alicia lives in Pune", axis(2)),
        ("Alicia works at Pune", axis(3)),
        ("Alice lives in Denver", axis(0)),
        ("Alice works at Pune", axis(1)),
        ("Alice", axis(0)),
        ("Alicia", axis(4)),
        ("Pune", axis(5)),
        ("Denver", axis(6)),
    ])));
    let alicia = remember_at(
        &engine,
        "Alicia lives in Pune",
        vec![
            EntityIn {
                name: "Alicia".into(),
                kind: Some("org".into()),
                ..Default::default()
            },
            named("Pune"),
        ],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alicia", "lives in", "Pune")
        }],
        T1,
    );
    let alice = remember_at(
        &engine,
        "Alice lives in Denver",
        vec![named("Alice"), named("Denver")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alice", "lives in", "Denver")
        }],
        T2,
    );
    assert_ne!(
        alicia.entities[0].id, alice.entities[0].id,
        "the two entities start apart"
    );
    // Both entities also describe the same non-exclusive fact.
    let dup_a = remember_at(
        &engine,
        "Alicia works at Pune",
        vec![],
        vec![rel("Alicia", "works at", "Pune")],
        T1,
    );
    let dup_b = remember_at(
        &engine,
        "Alice works at Pune",
        vec![],
        vec![rel("Alice", "works at", "Pune")],
        T1,
    );
    assert_ne!(dup_a.relations[0].id, dup_b.relations[0].id);

    let keep = alice.entities[0].id;
    engine
        .merge_entities(
            &format!("e{keep}"),
            &format!("e{}", alicia.entities[0].id),
            false,
        )
        .unwrap();

    let live: Vec<(String, String, Option<i64>)> = {
        let mut st = engine
            .db
            .sql()
            .prepare(
                "SELECT p.canonical,d.name,r.valid_to FROM relations r
                 JOIN predicates p ON p.id=r.predicate_id
                 JOIN entities d ON d.id=r.dst
                 WHERE r.src=?1 AND r.expired_at IS NULL AND r.valid_to IS NULL",
            )
            .unwrap();
        let rows = st
            .query_map([keep], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    assert_eq!(live.len(), 2, "{live:?}");
    assert!(live.iter().any(|(p, d, _)| p == "live in" && d == "Denver"));
    assert!(live.iter().any(|(p, d, _)| p == "work at" && d == "Pune"));
    let pune: (Option<i64>, Option<i64>, i64) = engine
        .db
        .sql()
        .query_row(
            "SELECT valid_to,expired_at,evidence_count FROM relations
             WHERE src=?1 AND dst=(SELECT id FROM entities WHERE name='Pune')
               AND predicate_id=(SELECT id FROM predicates WHERE canonical='live in')",
            [keep],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(pune.0, Some(T2), "the older object ends at the newer start");
    assert_eq!(pune.1, None);
    let evidence: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT evidence_count FROM relations
             WHERE src=?1 AND dst=(SELECT id FROM entities WHERE name='Pune')
               AND predicate_id=(SELECT id FROM predicates WHERE canonical='work at')",
            [keep],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(evidence, 2, "duplicate facts merge their evidence");
    let problems = engine.validate().unwrap();
    assert!(problems.is_empty(), "{problems:?}");
}

/// #10: a relation interval that cannot be true is rejected, naming it.
#[test]
fn relation_valid_to_must_be_after_valid_from() {
    let engine = engine();
    let mut r = rel("A", "lives in", "B");
    r.valid_from = Some(T1.to_string());
    r.valid_to = Some((T1 - 1).to_string());
    let error = engine
        .remember(RememberInput {
            content: "A lives in B".into(),
            entities: vec![named("A"), named("B")],
            relations: vec![r],
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("valid_to must be after valid_from"),
        "{error}"
    );
    assert!(
        error.contains("A lives in B"),
        "must name the relation: {error}"
    );
    assert_eq!(count(&engine, "SELECT count(*) FROM relations"), 0);
}

/// #14: a bare id is ambiguous for invalidate (it silently meant a memory).
#[test]
fn invalidate_rejects_a_bare_id_but_forget_accepts_one() {
    let engine = engine();
    let first = remember(&engine, "a memory", vec![named("A")], vec![]);
    let error = engine.invalidate("1", None).unwrap_err().to_string();
    assert!(error.contains("ambiguous"), "{error}");
    assert!(
        error.contains("m<id>") && error.contains("r<id>"),
        "{error}"
    );
    engine.invalidate("m1", None).unwrap();
    engine.forget(&format!("{}", first.id), false).unwrap();
}

/// #15: a hash hit whose kind/text differ is a collision, not a duplicate.
#[test]
fn a_hash_collision_with_other_text_is_not_a_duplicate() {
    let engine = engine();
    let content = "the staging host runs Debian";
    let hash = neocortex::text::content_hash("note", content);
    engine
        .db
        .write(|db| {
            db.sql().execute(
                "INSERT INTO memories(content,kind,created_at,valid_from,content_hash)
                 VALUES ('a different sentence','note',0,0,?1)",
                [&hash],
            )?;
            Ok(())
        })
        .unwrap();
    let out = engine
        .remember(RememberInput {
            content: content.into(),
            ..Default::default()
        })
        .unwrap();
    assert!(out.duplicate_of.is_none(), "{out:?}");
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM memories WHERE expired_at IS NULL"
        ),
        2
    );
}

/// #22: a hard-deleted memory must not leave `superseded by m<gone>` behind.
#[test]
fn hard_forget_clears_superseded_by_references() {
    let engine = engine();
    let old = remember(&engine, "the deploy day is Friday", vec![], vec![]);
    let new = engine
        .remember(RememberInput {
            content: "the deploy day is Thursday".into(),
            supersedes: vec![format!("m{}", old.id)],
            ..Default::default()
        })
        .unwrap();
    engine.forget(&format!("m{}", new.id), true).unwrap();
    let (superseded_by, expired_at): (Option<i64>, Option<i64>) = engine
        .db
        .sql()
        .query_row(
            "SELECT superseded_by,expired_at FROM memories WHERE id=?1",
            [old.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(superseded_by, None);
    assert!(expired_at.is_some(), "it stays history");
}

/// #3: opening a database must never chmod a directory it did not create.
#[cfg(unix)]
#[test]
fn opening_a_db_does_not_chmod_an_existing_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("chmod");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    drop(engine_at(&dir.join("mem.db")));
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "an existing directory must keep its mode");

    let created = dir.join("fresh").join("nested");
    drop(engine_at(&created.join("mem.db")));
    let mode = std::fs::metadata(&created).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "a directory this call created is private");
}

/// #17: a v1 database with duplicate live hashes migrates cleanly to v2.
#[test]
fn a_v1_database_with_duplicate_live_hashes_migrates_to_v2() {
    let dir = temp_dir("migrate");
    let path = dir.join("mem.db");
    {
        let engine = engine_at(&path);
        remember(&engine, "the deploy day is Friday", vec![], vec![]);
        remember(&engine, "the staging host runs Debian", vec![], vec![]);
    }
    let oldest;
    {
        // Rewind to the v1 shape: no schema_meta, a non-unique hash index and
        // two live rows that share a content hash.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE schema_meta;
             DROP INDEX idx_memories_hash;
             CREATE INDEX idx_memories_hash ON memories(content_hash) WHERE expired_at IS NULL;",
        )
        .unwrap();
        let hash: String = conn
            .query_row(
                "SELECT content_hash FROM memories WHERE content='the deploy day is Friday'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        oldest = conn
            .query_row(
                "SELECT id FROM memories WHERE content='the deploy day is Friday'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO memories(content,kind,created_at,valid_from,content_hash)
             VALUES ('the deploy day is Friday','note',0,0,?1)",
            [&hash],
        )
        .unwrap();
        conn.execute_batch("PRAGMA user_version=1").unwrap();
    }
    let engine = engine_at(&path);
    let live: i64 = count(
        &engine,
        "SELECT count(*) FROM memories WHERE expired_at IS NULL",
    );
    assert_eq!(live, 2, "one live copy per hash after migration");
    let newer: (Option<i64>, Option<i64>) = engine
        .db
        .sql()
        .query_row(
            "SELECT expired_at,superseded_by FROM memories
             WHERE content='the deploy day is Friday' AND expired_at IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(newer.0.is_some());
    assert_eq!(newer.1, Some(oldest), "the oldest copy is the survivor");
    let version: i64 = engine
        .db
        .sql()
        .query_row("SELECT version FROM schema_meta", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
    // The unique index is live now.
    let err = engine
        .db
        .sql()
        .execute(
            "INSERT INTO memories(content,kind,created_at,valid_from,content_hash)
             VALUES ('another copy','note',0,0,(SELECT content_hash FROM memories
                 WHERE content='the deploy day is Friday' AND expired_at IS NULL))",
            [],
        )
        .unwrap_err();
    assert!(neocortex::db::is_unique_violation(&err), "{err}");
}

#[test]
fn relation_endpoints_are_mentioned_by_their_memory() {
    let engine = engine();
    // Neither endpoint exists yet, so the gazetteer cannot link them.
    let out = remember(
        &engine,
        "Bob works at Acme.",
        vec![],
        vec![rel("Bob", "works at", "Acme")],
    );
    let mentioned: i64 = engine
        .db
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM mentions WHERE memory_id=?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mentioned, 2);
}

#[test]
fn a_v2_database_gains_polarity_and_seeded_predicates_on_open() {
    let dir = temp_dir("migrate-v3");
    let path = dir.join("mem.db");
    {
        let engine = engine_at(&path);
        remember(
            &engine,
            "Bob works at Acme",
            vec![],
            vec![rel("Bob", "works at", "Acme")],
        );
    }
    {
        // Rewind to the v2 shape: no polarity/ended_by, no seeded exclusivity.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "ALTER TABLE relations DROP COLUMN polarity;
             ALTER TABLE relations DROP COLUMN ended_by;
             UPDATE predicates SET exclusive=0;
             UPDATE schema_meta SET version=2;",
        )
        .unwrap();
    }
    let engine = engine_at(&path);
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM relations WHERE polarity=1 AND ended_by IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &engine,
            "SELECT exclusive FROM predicates WHERE canonical='work at'"
        ),
        1
    );
    assert_eq!(count(&engine, "SELECT version FROM schema_meta"), 4);
    let predicate_text: String = engine
        .db
        .sql()
        .query_row("SELECT predicate_text FROM relations WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(predicate_text, "works at");
    // The v3 -> v4 step rebuilt entities with the typed unique index.
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM pragma_index_info('idx_entities_norm_type') WHERE name='homonym'"
        ),
        1
    );
}

/// The bench caught this: the committed baseline stamps v3 with a three-column
/// `predicates` table (id, canonical, exclusive), so the v3 -> v4 step — not
/// the v2 step — has to survive every pre-f2 predicate shape.
#[test]
fn a_baseline_v3_database_with_three_column_predicates_opens() {
    let dir = temp_dir("migrate-baseline-v3");
    let path = dir.join("mem.db");
    {
        let engine = engine_at(&path);
        remember(
            &engine,
            "Bob works at Acme",
            vec![],
            vec![rel("Bob", "works at", "Acme")],
        );
    }
    {
        // Rewind `predicates` to the exact baseline v3 shape, keeping the
        // alias rows.
        let conn = rusqlite::Connection::open(&path).unwrap();
        let aliases: Vec<(String, i64)> = conn
            .prepare("SELECT alias_norm,predicate_id FROM predicate_aliases")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=OFF;
             DROP TABLE predicates;
             CREATE TABLE predicates (
               id INTEGER PRIMARY KEY,
               canonical TEXT NOT NULL UNIQUE,
               exclusive INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO predicates(canonical,exclusive) VALUES ('work at',1);
             UPDATE relations SET predicate_id=1, predicate_text='', subj=0;
             ALTER TABLE relations DROP COLUMN predicate_text;
             ALTER TABLE relations DROP COLUMN subj;
             UPDATE schema_meta SET version=3;",
        )
        .unwrap();
        for (alias, id) in aliases {
            conn.execute(
                "INSERT OR IGNORE INTO predicate_aliases(alias_norm,predicate_id) VALUES (?1,?2)",
                rusqlite::params![alias, id],
            )
            .unwrap();
        }
    }
    let engine = engine_at(&path);
    assert_eq!(count(&engine, "SELECT version FROM schema_meta"), 4);
    // The ontology is re-seeded on the extended table.
    assert_eq!(
        count(
            &engine,
            "SELECT exclusive FROM predicates WHERE canonical='work at'"
        ),
        1
    );
    assert_eq!(
        count(
            &engine,
            "SELECT count(*) FROM predicates WHERE display!='' AND group_id!=0"
        ),
        16,
        "the full ontology is re-seeded on the extended table"
    );
    // The old relation kept its meaning and its rendered text.
    let (text, live): (String, i64) = engine
        .db
        .sql()
        .query_row(
            "SELECT predicate_text,polarity FROM relations WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(text, "works at");
    assert_eq!(live, 1);
    let a: i64 = engine
        .db
        .sql()
        .query_row("SELECT count(*) FROM entities", [], |r| r.get(0))
        .unwrap();
    assert_eq!(a, 2);
}

// ================================================================== f2

fn typed(name: &str, kind: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some(kind.into()),
        ..Default::default()
    }
}

/// er.md ER-01 / query.md ER-05: the same name with a different explicit type
/// (or only differently-cased) must never abort the write.
#[test]
fn same_name_different_types_coexist_and_type_casing_folds() {
    let e = engine();
    let talk = remember(
        &e,
        "Priya gave a talk.",
        vec![typed("Priya", "person")],
        vec![],
    );
    let chai = remember(
        &e,
        "Priya likes chai.",
        vec![typed("Priya", "Person")],
        vec![],
    );
    assert_eq!(chai.entities[0].status, "matched");
    assert_eq!(chai.entities[0].id, talk.entities[0].id);
    let place = remember(
        &e,
        "Priya is in Pune.",
        vec![typed("Priya", "place")],
        vec![],
    );
    assert_eq!(place.entities[0].status, "new");
    assert_ne!(place.entities[0].id, talk.entities[0].id);
    assert_eq!(
        count(&e, "SELECT count(*) FROM entities WHERE norm='priya'"),
        2
    );
    // validate stays clean with homonyms in the graph.
    assert!(e.validate().unwrap().is_empty());
}

#[test]
fn untyped_mention_matches_a_single_entity_with_that_name() {
    let e = engine();
    let talk = remember(
        &e,
        "Priya gave a talk.",
        vec![typed("Priya", "person")],
        vec![],
    );
    let chai = remember(&e, "Priya likes chai.", vec![named("Priya")], vec![]);
    assert_eq!(chai.entities[0].status, "matched");
    assert_eq!(chai.entities[0].id, talk.entities[0].id);
}

#[test]
fn untyped_mention_among_homonyms_is_ambiguous_and_never_loses_the_memory() {
    let e = engine();
    remember(
        &e,
        "Priya gave a talk.",
        vec![typed("Priya", "person")],
        vec![],
    );
    remember(
        &e,
        "Priya is a place name.",
        vec![typed("Priya", "place")],
        vec![],
    );
    let out = remember(&e, "Priya came up again.", vec![named("Priya")], vec![]);
    let res = &out.entities[0];
    assert_eq!(res.status, "ambiguous");
    assert_eq!(res.candidates.len(), 2, "{res:?}");
    // The memory was stored anyway.
    let stored: i64 =
        e.db.sql()
            .query_row("SELECT count(*) FROM memories WHERE id=?1", [out.id], |r| {
                r.get(0)
            })
            .unwrap();
    assert_eq!(stored, 1);
}

#[test]
fn distinct_from_forces_a_new_homonym_and_a_bounce_back_is_ambiguous() {
    use neocortex::engine::EntityIn;
    let e = engine();
    let first = remember(
        &e,
        "Alex the person spoke.",
        vec![typed("Alex", "person")],
        vec![],
    );
    let forced = remember(
        &e,
        "Alex spoke again, differently.",
        vec![EntityIn {
            kind: Some("person".into()),
            distinct_from: vec![format!("e{}", first.entities[0].id)],
            ..named("Alex")
        }],
        vec![],
    );
    assert_eq!(forced.entities[0].status, "new");
    assert_ne!(forced.entities[0].id, first.entities[0].id);
    let homonym: (i64, i64) =
        e.db.sql()
            .query_row(
                "SELECT count(*), (SELECT homonym FROM entities WHERE id=?1)
             FROM entities WHERE norm='alex' AND type='person'",
                [forced.entities[0].id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(homonym, (2, 1), "second (norm,type) row, homonym 1");
    // A plain mention now sees two typed homonyms: ambiguous, never merged.
    let bounce = remember(&e, "Alex is around.", vec![named("Alex")], vec![]);
    let res = &bounce.entities[0];
    assert_eq!(res.status, "ambiguous");
    assert_eq!(res.candidates.len(), 2, "{res:?}");
}

#[test]
fn cross_type_merge_needs_force() {
    let e = engine();
    let person = remember(&e, "Jhon presented.", vec![typed("Jhon", "person")], vec![]);
    let org = remember(
        &e,
        "Acme builds things.",
        vec![typed("Acme", "organization")],
        vec![],
    );
    let err = e
        .merge_entities(
            &format!("e{}", person.entities[0].id),
            &format!("e{}", org.entities[0].id),
            false,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("different types"), "{err}");
    e.merge_entities(
        &format!("e{}", person.entities[0].id),
        &format!("e{}", org.entities[0].id),
        true,
    )
    .unwrap();
    assert_eq!(count(&e, "SELECT count(*) FROM entities"), 1);
}

#[test]
fn merge_unknown_id_is_a_domain_error() {
    let e = engine();
    remember(&e, "Alex spoke.", vec![named("Alex")], vec![]);
    let err = e
        .merge_entities("e1", "e99", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown entity"), "{err}");
}

/// er.md ER-06: relations that become self-loops are reported and kept as
/// expired rows, never silently deleted.
#[test]
fn merge_reports_self_loop_relation_and_keeps_it_expired() {
    let e = engine();
    let person = remember(&e, "Jhon presented.", vec![typed("Jhon", "person")], vec![]);
    remember(
        &e,
        "Jhon works at Acme.",
        vec![typed("Acme", "organization")],
        vec![rel("Jhon", "works at", "Acme")],
    );
    let out = e
        .merge_entities(&format!("e{}", person.entities[0].id), "e2", true)
        .unwrap();
    let report = out.changed.join(" | ");
    assert!(report.contains("self-loop"), "{report}");
    // The relation row survives as history.
    let (expired, valid_to): (Option<i64>, Option<i64>) =
        e.db.sql()
            .query_row(
                "SELECT expired_at,valid_to FROM relations WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert!(expired.is_some());
    assert!(
        valid_to.is_none(),
        "retracted, not given an invented world end"
    );
    // It is out of the live graph and the projection stays exact.
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM relations WHERE expired_at IS NULL"
        ),
        0
    );
    assert!(e.validate().unwrap().is_empty());
}

// rel.md ER-20: merge reconciliation ends the older exclusive fact at the
// newer start without recording a zero-length world interval.
#[test]
fn merge_reconciles_exclusive_facts_without_zero_length_intervals() {
    let e = engine();
    let austin = remember_at(
        &e,
        "Alex lives in Austin",
        vec![named("Alex"), named("Austin")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alex", "lives in", "Austin")
        }],
        T1,
    );
    let denver = remember_at(
        &e,
        "Alexis lives in Denver",
        vec![named("Alexis"), named("Denver")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alexis", "lives in", "Denver")
        }],
        T2,
    );
    let keep = denver.entities[0].id;
    e.merge_entities(
        &format!("e{keep}"),
        &format!("e{}", austin.entities[0].id),
        false,
    )
    .unwrap();
    let (from, to, expired): (i64, Option<i64>, Option<i64>) =
        e.db.sql()
            .query_row(
                "SELECT valid_from,valid_to,expired_at FROM relations
             WHERE dst=(SELECT id FROM entities WHERE norm='austin')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    assert_eq!(
        Some(T2),
        to,
        "the older fact ends at the newer start (not its own start)"
    );
    assert!(to.unwrap() > from, "non-zero interval");
    assert!(expired.is_none(), "true in the past, no system retirement");
    // Equal starts must not fabricate a zero-length interval either.
    let a2 = remember_at(
        &e,
        "Alex2 lives in Austin",
        vec![named("Alex2"), named("Austin")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alex2", "lives in", "Austin")
        }],
        T1,
    );
    let d2 = remember_at(
        &e,
        "Alexis2 lives in Denver",
        vec![named("Alexis2"), named("Denver")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Alexis2", "lives in", "Denver")
        }],
        T1,
    );
    let keep2 = d2.entities[0].id;
    e.merge_entities(
        &format!("e{keep2}"),
        &format!("e{}", a2.entities[0].id),
        false,
    )
    .unwrap();
    let (to2, expired2): (Option<i64>, Option<i64>) =
        e.db.sql()
            .query_row(
                "SELECT valid_to,expired_at FROM relations WHERE id=?1",
                [a2.relations[0].id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert!(to2.is_none(), "no zero-length interval ({to2:?})");
    assert!(expired2.is_some(), "equal-start loser is system-retired");
}

/// rel.md ER-10 / rel.md ER-12: an inverse or symmetric write is stored once
/// in the canonical direction and rendered from both sides.
#[test]
fn inverse_and_symmetric_relations_render_from_both_sides() {
    let e = engine();
    remember(
        &e,
        "Iris reports to Kofi.",
        vec![named("Iris"), named("Kofi")],
        vec![rel("Iris", "reports to", "Kofi")],
    );
    remember(
        &e,
        "Iris is married to Kofi.",
        vec![named("Iris"), named("Kofi")],
        vec![rel("Iris", "married to", "Kofi")],
    );
    let (iris, kofi) = {
        let iris = entity_id(&e, "Iris");
        let kofi = entity_id(&e, "Kofi");
        (iris, kofi)
    };
    // Stored once, canonical direction: Kofi manages Iris.
    let (pred, src, dst): (String, i64, i64) = e
        .db
        .sql()
        .query_row(
            "SELECT p.canonical,r.src,r.dst FROM relations r JOIN predicates p ON p.id=r.predicate_id
             WHERE p.canonical='manage'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(pred, "manage");
    assert_eq!(src, kofi, "src is the manager (Kofi manages Iris)");
    assert_eq!(dst, iris);
    // The managed side's card renders the inverse phrase with itself as
    // subject; the wrong-direction phrase never appears.
    let card = e.entity(&format!("e{iris}"), 20).unwrap();
    let text = neocortex::render::render_entity(&card, &|t| t.to_string());
    assert!(text.contains("Iris —reports to→ Kofi"), "{text}");
    assert!(!text.contains("Iris —manage"), "{text}");
    // ...and the manager's card reads the stored direction.
    let card = e.entity(&format!("e{kofi}"), 20).unwrap();
    let text = neocortex::render::render_entity(&card, &|t| t.to_string());
    assert!(text.contains("Kofi —manages→ Iris"), "{text}");
    assert!(!text.contains("Kofi —reports to"), "{text}");
}

#[test]
fn symmetric_writes_merge_into_one_row() {
    let e = engine();
    remember(
        &e,
        "Iris is married to Kofi.",
        vec![named("Iris"), named("Kofi")],
        vec![rel("Iris", "married to", "Kofi")],
    );
    let second = remember(
        &e,
        "Kofi is married to Iris.",
        vec![named("Kofi"), named("Iris")],
        vec![rel("Kofi", "married to", "Iris")],
    );
    assert_eq!(count(&e, "SELECT count(*) FROM relations"), 1);
    assert_eq!(second.relations[0].id, 1, "merged as evidence");
    assert_eq!(
        count(&e, "SELECT evidence_count FROM relations WHERE id=1"),
        2
    );
}

/// rel.md ER-17: an empty predicate is rejected with an actionable error.
#[test]
fn empty_predicate_is_rejected() {
    let e = engine();
    let err = e
        .remember(RememberInput {
            content: "Alice relates to Bob.".into(),
            entities: vec![named("Alice"), named("Bob")],
            relations: vec![rel("Alice", "", "Bob")],
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("predicate cannot be empty"), "{err}");
}

/// rel.md ER-16: `exclusive: false` may clear sticky exclusivity, but only on
/// a predicate the agent created (never on the seeded ontology).
#[test]
fn exclusive_false_clears_sticky_exclusivity_only_for_unseeded() {
    let e = engine();
    // "likes" is agent-created: one explicit true, then one false write,
    // and a later omitted write no longer ends anything.
    let first = remember(
        &e,
        "Bob likes Acme.",
        vec![named("Bob"), named("Acme")],
        vec![RelationIn {
            exclusive: Some(true),
            ..rel("Bob", "likes", "Acme")
        }],
    );
    let keep = remember(
        &e,
        "Bob also likes Globex.",
        vec![named("Globex")],
        vec![RelationIn {
            exclusive: Some(false),
            ..rel("Bob", "likes", "Globex")
        }],
    );
    let sticky: i64 =
        e.db.sql()
            .query_row(
                "SELECT exclusive FROM predicates WHERE canonical='like'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(sticky, 0, "the unseeded flag was cleared");
    let again = remember(
        &e,
        "Bob likes Zenith.",
        vec![named("Zenith")],
        vec![rel("Bob", "likes", "Zenith")],
    );
    assert!(again.invalidated_relations.is_empty(), "{again:?}",);
    let _ = (first, keep);
    // Seeded "works at" keeps its policy: the false write is per-write only.
    let e = engine();
    remember(
        &e,
        "Bob works at Acme.",
        vec![named("Acme")],
        vec![rel("Bob", "works at", "Acme")],
    );
    remember(
        &e,
        "Bob also works at Globex.",
        vec![named("Globex")],
        vec![RelationIn {
            exclusive: Some(false),
            ..rel("Bob", "works at", "Globex")
        }],
    );
    let again = remember(
        &e,
        "Bob works at Zenith.",
        vec![named("Zenith")],
        vec![rel("Bob", "works at", "Zenith")],
    );
    assert_eq!(
        again.invalidated_relations.len(),
        2,
        "seeded policy still ends the older values"
    );
}

/// A same-name same-type homonym pool is separated by what the memory is
/// about (context), not only by the bare mention card: the winning candidate
/// still keeps the write ambiguous, but it is listed first with its score.
#[test]
fn context_prefers_the_candidate_the_memory_is_about() {
    use neocortex::engine::EntityIn;
    let e = engine();
    let baker = remember(
        &e,
        "Alex ought to have pastry in the night sky? no; Alex bakes bread for the market.",
        vec![typed("Alex", "person")],
        vec![],
    );
    let engineer = remember(
        &e,
        "Another entry: Alex engineers elevators in a different city.",
        vec![EntityIn {
            distinct_from: vec![format!("e{}", baker.entities[0].id)],
            ..typed("Alex", "person")
        }],
        vec![],
    );
    assert_ne!(engineer.entities[0].id, baker.entities[0].id);
    let who = remember(
        &e,
        "Alex bakes bread again today.",
        vec![named("Alex")],
        vec![],
    );
    let res = &who.entities[0];
    assert_eq!(res.status, "ambiguous");
    assert_eq!(res.candidates.len(), 2, "{res:?}");
    let winner = res.candidates[0].score.unwrap_or(0.0);
    let runner_up = res.candidates[1].score.unwrap_or(0.0);
    assert!(winner > runner_up, "{res:?}");
    assert_eq!(res.candidates[0].id, baker.entities[0].id, "{res:?}");
}

/// Model-gated: a same-name mention carrying a conflicting description keeps
/// the matched entity but reports `ambiguous` with the candidate, then
/// `distinct_from` forces the split (er.md ER-04, hunt/x_er.md D2).
#[test]
fn conflicting_description_reports_ambiguous_then_distinct_from_splits() {
    let dir = match std::env::var_os("NEOCORTEX_MODEL_DIR") {
        Some(p) => std::path::PathBuf::from(p),
        None => home_models(),
    };
    if !dir.is_dir() {
        eprintln!("skipping: model directory absent: {}", dir.display());
        return;
    }
    let emb = neocortex::embed::load(Some(&dir)).unwrap();
    let e = engine_with(Arc::from(emb) as Arc<dyn Embedder>);
    let first = remember(
        &e,
        "Alex note.",
        vec![EntityIn {
            description: Some("backend engineer at Stripe in Boston".into()),
            ..typed("Alex", "person")
        }],
        vec![],
    );
    let second = remember(
        &e,
        "Alex elsewhere.",
        vec![EntityIn {
            description: Some("runs a bakery in Chicago".into()),
            ..typed("Alex", "person")
        }],
        vec![],
    );
    let res = &second.entities[0];
    // Measured on UltraBERT (examples/measure_desc.rs, 22 + 22 pairs):
    // same-person min 0.7404, different-entity max 0.7275 -> 0.73 separates.
    assert_eq!(res.status, "ambiguous", "{res:?}");
    assert_eq!(res.candidates.len(), 1);
    assert_eq!(res.candidates[0].id, first.entities[0].id);
    // The description was NOT concatenated into the stored card.
    let card: String =
        e.db.sql()
            .query_row(
                "SELECT description FROM entities WHERE id=?1",
                [first.entities[0].id],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(card, "backend engineer at Stripe in Boston");
    // Re-sending with distinct_from splits the homonym, memory intact.
    let third = remember(
        &e,
        "Alex elsewhere.",
        vec![EntityIn {
            description: Some("runs a bakery in Chicago".into()),
            distinct_from: vec![format!("e{}", first.entities[0].id)],
            ..typed("Alex", "person")
        }],
        vec![],
    );
    assert_eq!(third.entities[0].status, "new");
    assert_ne!(third.entities[0].id, first.entities[0].id);
    assert!(e.validate().unwrap().is_empty());
}

fn home_models() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(|h| std::path::Path::new(&h).join(".neocortex/models/UltraBERT-Embed-150M"))
        .unwrap_or_else(|| std::path::PathBuf::from(".neocortex/models/UltraBERT-Embed-150M"))
}

#[test]
fn text_with_several_entities_and_no_relations_asks_for_relations() {
    let engine = engine();
    let out = remember(
        &engine,
        "Sarah Chen joined Acme Corp in Austin.",
        vec![named("Sarah Chen"), named("Acme Corp")],
        vec![],
    );
    assert!(
        out.hints.iter().any(|h| h.starts_with("no facts stored")),
        "{:?}",
        out.hints
    );
    let out = remember(
        &engine,
        "Sarah Chen works at Acme Corp.",
        vec![],
        vec![rel("Sarah Chen", "works at", "Acme Corp")],
    );
    assert!(
        !out.hints.iter().any(|h| h.starts_with("no facts stored")),
        "{:?}",
        out.hints
    );
}
