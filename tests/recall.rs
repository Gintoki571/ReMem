use neocortex::embed::{Embedder, HashEmbedder, Mode, DIMS};
use neocortex::engine::{
    Config, Engine, EntityIn, RecallInput, RecallOutput, RelationIn, RememberInput,
};
use neocortex::paths;
use neocortex::temporal::{parse_instant, Clock};
use neocortex::Result;
use std::collections::HashMap;
use std::sync::Arc;

fn clock() -> Clock {
    Clock {
        now: parse_instant(
            "2026-10-21T15:00:00Z",
            &Clock {
                now: 0,
                tz_offset_secs: 0,
            },
        )
        .unwrap(),
        tz_offset_secs: 0,
    }
}

fn engine(emb: Arc<dyn Embedder>) -> Engine {
    Engine::open_in_memory(emb).unwrap().with_clock(clock())
}

fn remember(engine: &Engine, content: &str, setup: impl FnOnce(&mut RememberInput)) -> i64 {
    let mut input = RememberInput {
        content: content.into(),
        ..Default::default()
    };
    setup(&mut input);
    engine.remember(input).unwrap().id
}

fn entity(name: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        ..Default::default()
    }
}

fn relation(source: &str, predicate: &str, target: &str, fact: &str) -> RelationIn {
    RelationIn {
        source: source.into(),
        predicate: predicate.into(),
        target: target.into(),
        fact: Some(fact.into()),
        ..Default::default()
    }
}

fn ids(out: &RecallOutput) -> Vec<i64> {
    let mut v: Vec<i64> = out.memories.iter().map(|m| m.id).collect();
    v.sort_unstable();
    v
}

fn has_reason(out: &RecallOutput, id: i64, prefix: &str) -> bool {
    out.memories
        .iter()
        .find(|m| m.id == id)
        .is_some_and(|m| m.reasons.iter().any(|r| r.starts_with(prefix)))
}

#[test]
fn keyword_recall_returns_matching_memory() {
    let engine = Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(Clock {
            now: 1_800_000_000,
            tz_offset_secs: 0,
        });
    engine
        .remember(RememberInput {
            content: "The production database runs PostgreSQL".into(),
            ..Default::default()
        })
        .unwrap();
    let out = engine
        .recall(&RecallInput {
            query: "production database".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.memories.len(), 1);
    assert!(out.memories[0].content.contains("PostgreSQL"));
}

#[test]
fn filters_apply_to_kind_tags_scope_and_validity() {
    let e = engine(Arc::new(HashEmbedder));
    let event_work = remember(&e, "deployment checklist for the ops rotation", |i| {
        i.kind = Some("event".into());
        i.tags = vec!["ops".into()];
        i.scope = Some("work".into());
        i.valid_from = Some("2026-01-05".into());
    });
    let fact_work = remember(&e, "deployment checklist for the database rotation", |i| {
        i.kind = Some("fact".into());
        i.tags = vec!["ops".into(), "db".into()];
        i.scope = Some("work".into());
        i.valid_from = Some("2026-06-01".into());
    });
    let event_home = remember(&e, "deployment checklist for the home network", |i| {
        i.kind = Some("event".into());
        i.tags = vec!["home".into()];
        i.scope = Some("personal".into());
        i.valid_from = Some("2026-09-01".into());
    });
    let recall = |input: RecallInput| ids(&e.recall(&input).unwrap());

    let kinds = recall(RecallInput {
        query: "deployment checklist".into(),
        kinds: vec!["event".into()],
        ..Default::default()
    });
    assert_eq!(kinds, vec![event_work, event_home]);

    let tags = recall(RecallInput {
        query: "deployment checklist".into(),
        tags: vec!["db".into()],
        ..Default::default()
    });
    assert_eq!(tags, vec![fact_work]);

    let scope = recall(RecallInput {
        query: "deployment checklist".into(),
        scope: Some("personal".into()),
        ..Default::default()
    });
    assert_eq!(scope, vec![event_home]);

    // Interval overlap: the open-ended January event still holds in May.
    let mut since = recall(RecallInput {
        query: "deployment checklist".into(),
        since: Some("2026-05-01".into()),
        ..Default::default()
    });
    since.sort();
    assert_eq!(since, vec![event_work, fact_work, event_home]);

    let until = recall(RecallInput {
        query: "deployment checklist".into(),
        until: Some("2026-06-01".into()),
        ..Default::default()
    });
    assert_eq!(until, vec![event_work]);
}

#[test]
fn as_of_returns_the_then_valid_memory() {
    let e = engine(Arc::new(HashEmbedder));
    let delhi = remember(&e, "The office is in Delhi", |i| {
        i.valid_from = Some("2025-01-01".into());
        i.valid_to = Some("2026-01-01".into());
    });
    let pune = remember(&e, "The office is in Pune", |i| {
        i.valid_from = Some("2026-01-01".into());
    });
    let out = e
        .recall(&RecallInput {
            query: "office".into(),
            as_of: Some("2025-06-01".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ids(&out), vec![delhi]);
    assert!(!ids(&out).contains(&pune));
}

#[test]
fn as_of_graph_excludes_relations_that_start_in_the_future() {
    let e = engine(Arc::new(HashEmbedder));
    let answer = remember(&e, "Anika owns the audit binder.", |i| {
        i.entities = vec![entity("Anika")];
    });
    remember(&e, "Priya mentors Anika.", |i| {
        i.entities = vec![entity("Priya"), entity("Anika")];
        i.relations = vec![RelationIn {
            source: "Priya".into(),
            predicate: "mentors".into(),
            target: "Anika".into(),
            fact: Some("Priya mentors Anika".into()),
            valid_from: Some("2027-01-01".into()),
            ..Default::default()
        }];
    });
    let out = e
        .recall(&RecallInput {
            query: "What does Priya oversee?".into(),
            as_of: Some("2026-10-01".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(!has_reason(&out, answer, "graph#"));
}

#[test]
fn temporal_channel_scores_all_small_window_matches_not_only_newest_400() {
    let e = engine(Arc::new(HashEmbedder));
    let old = remember(&e, "quarterly archive marker", |i| {
        i.valid_from = Some("1577836800".into());
    });
    for day in 1..405 {
        remember(&e, &format!("quarterly archive filler {day}"), |i| {
            i.valid_from = Some((1577836800_i64 + day * 86_400).to_string());
        });
    }
    let out = e
        .recall(&RecallInput {
            query: "in 2020 quarterly archive marker".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&old));
}

#[test]
fn superseded_hidden_normally_returned_with_history() {
    let e = engine(Arc::new(HashEmbedder));
    let old = remember(&e, "The office is in Delhi", |_| {});
    let new = remember(&e, "The office is in Pune", |i| {
        i.supersedes = vec![format!("m{old}")];
    });

    let normal = e
        .recall(&RecallInput {
            query: "office".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ids(&normal), vec![new]);
    assert!(!normal.memories[0].ended);

    let history = e
        .recall(&RecallInput {
            query: "office".into(),
            include_history: true,
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&history).contains(&old));
    let old_hit = history.memories.iter().find(|m| m.id == old).unwrap();
    assert!(old_hit.ended);

    let used_to = e
        .recall(&RecallInput {
            query: "where did the office used to be".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&used_to).contains(&old));
}

#[test]
fn ended_memory_ranks_below_current_without_history() {
    let e = engine(Arc::new(HashEmbedder));
    let current = remember(&e, "The team laptop stays in the Pune office", |i| {
        i.valid_from = Some("2026-01-01".into());
    });
    let ended = remember(&e, "The team laptop stays in the Delhi office", |i| {
        i.valid_from = Some("2026-01-01".into());
        i.valid_to = Some("2026-05-01".into());
    });
    let out = e
        .recall(&RecallInput {
            query: "team laptop office".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.memories[0].id, current);
    let ended_hit = out.memories.iter().find(|m| m.id == ended).unwrap();
    assert!(ended_hit.ended);
    let current_pos = out.memories.iter().position(|m| m.id == current).unwrap();
    let ended_pos = out.memories.iter().position(|m| m.id == ended).unwrap();
    assert!(current_pos < ended_pos);
}

#[test]
fn forgotten_memory_is_never_returned() {
    let e = engine(Arc::new(HashEmbedder));
    let id = remember(&e, "The vault code is 4417", |_| {});
    e.forget(&format!("m{id}"), false).unwrap();
    let normal = e
        .recall(&RecallInput {
            query: "vault code".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(normal.memories.is_empty());
    let history = e
        .recall(&RecallInput {
            query: "vault code".into(),
            include_history: true,
            ..Default::default()
        })
        .unwrap();
    assert!(history.memories.is_empty());
}

#[test]
fn multi_hop_reaches_memory_that_only_names_the_far_entity() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Priya leads the platform team.", |i| {
        i.entities = vec![entity("Priya")];
    });
    let answer = remember(&e, "Anika owns the quarterly audit binder.", |i| {
        i.entities = vec![entity("Anika")];
    });
    remember(&e, "Priya mentors Anika.", |i| {
        i.entities = vec![entity("Priya"), entity("Anika")];
        i.relations = vec![relation("Priya", "mentors", "Anika", "Priya mentors Anika")];
    });
    let out = e
        .recall(&RecallInput {
            query: "What does Priya oversee?".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&answer));
    assert!(has_reason(&out, answer, "graph#"));
}

#[test]
fn two_hop_path_answers_with_the_manager_home_city() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Noah Tansen's manager is Mira Shah.", |i| {
        i.entities = vec![entity("Noah Tansen"), entity("Mira Shah")];
        i.relations = vec![relation(
            "Noah Tansen",
            "manager",
            "Mira Shah",
            "Noah Tansen's manager is Mira Shah.",
        )];
    });
    let answer = remember(&e, "Mira Shah lives in Oslo.", |i| {
        i.entities = vec![entity("Mira Shah"), entity("Oslo")];
        i.relations = vec![relation(
            "Mira Shah",
            "lives in",
            "Oslo",
            "Mira Shah lives in Oslo.",
        )];
    });
    let out = e
        .recall(&RecallInput {
            query: "where does the manager of Noah Tansen live".into(),
            k: Some(5),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&answer), "{:?}", out.memories);
    assert!(
        out.facts.iter().any(|f| f.target == "Oslo"),
        "the last hop must render, got {:?}",
        out.facts
    );
}

#[test]
fn three_hop_path_covers_sibling_manager_and_city() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Elsa's sister is Maria.", |i| {
        i.entities = vec![entity("Elsa"), entity("Maria")];
        i.relations = vec![relation(
            "Elsa",
            "sibling of",
            "Maria",
            "Elsa's sister is Maria.",
        )];
    });
    remember(&e, "Liam is Maria's manager.", |i| {
        i.entities = vec![entity("Liam"), entity("Maria")];
        i.relations = vec![relation(
            "Liam",
            "manager",
            "Maria",
            "Liam is Maria's manager.",
        )];
    });
    let answer = remember(&e, "Liam's home is in Denver.", |i| {
        i.entities = vec![entity("Liam"), entity("Denver")];
        i.relations = vec![relation(
            "Liam",
            "lives in",
            "Denver",
            "Liam's home is in Denver.",
        )];
    });
    let out = e
        .recall(&RecallInput {
            query: "what city is home to the manager of Elsa's sister".into(),
            k: Some(5),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&answer), "{:?}", out.memories);
    assert!(
        out.facts.iter().any(|f| f.target == "Denver"),
        "the last hop must render, got {:?}",
        out.facts
    );
}

#[test]
fn negation_question_returns_only_negative_memories() {
    let e = engine(Arc::new(HashEmbedder));
    for name in ["Priya", "Anika"] {
        remember(&e, &format!("{name} likes jazz."), |i| {
            i.entities = vec![entity(name)];
        });
    }
    let negative = remember(&e, "Bob does not like jazz.", |i| {
        i.entities = vec![entity("Bob")];
    });
    let out = e
        .recall(&RecallInput {
            query: "who does not like jazz".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&negative), "{:?}", out.memories);
    assert!(
        out.memories.iter().all(|m| m.content.contains("not")),
        "positive facts leaked into a negation answer: {:?}",
        out.memories.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
    assert!(out.facts.iter().all(|f| f.fact.contains("not")));
}

#[test]
fn negation_question_keeps_negative_relations_without_a_cue_word() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Priya likes jazz.", |i| {
        i.relations = vec![relation("Priya", "likes", "jazz", "Priya likes jazz")];
    });
    let negative = remember(&e, "Bob refuses to listen to jazz.", |i| {
        i.relations = vec![RelationIn {
            negated: Some(true),
            ..relation("Bob", "likes", "jazz", "")
        }];
    });
    let out = e
        .recall(&RecallInput {
            query: "who does not like jazz".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ids(&out), vec![negative], "{:?}", out.memories);
    assert!(out.facts.iter().all(|f| f.polarity == 0), "{:?}", out.facts);
}

#[test]
fn a_chain_link_stated_only_in_prose_is_followed() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Rui Carvalho is mentored by Marta Lobo.", |i| {
        i.relations = vec![relation(
            "Marta Lobo",
            "mentors",
            "Rui Carvalho",
            "Marta Lobo mentors Rui Carvalho",
        )];
    });
    for filler in [
        "Rui Carvalho runs the Saturday football drills.",
        "Rui Carvalho prefers espresso over filter coffee.",
        "Rui Carvalho presented the assay results.",
        "Rui Carvalho bought a new bicycle.",
        "Marta Lobo reviewed the grant budget.",
        "Marta Lobo chairs the hiring panel.",
    ] {
        remember(&e, filler, |_| {});
    }
    // No relations passed: the hometown link exists only as prose.
    let answer = remember(&e, "Marta Lobo spent her childhood in Braga.", |_| {});
    let out = e
        .recall(&RecallInput {
            query: "Which town did Rui Carvalho's mentor grow up in?".into(),
            ..Default::default()
        })
        .unwrap();
    // Only the semantic beam's prose hop reaches it as the best chain.
    let hit = out.memories.iter().find(|m| m.id == answer);
    assert!(
        hit.is_some_and(|m| m.reasons.iter().any(|r| r == "sem#1")),
        "{:?}",
        out.memories
    );
}

#[test]
fn unrelated_question_abstains_instead_of_ranking_noise() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "The office coffee machine is in the kitchen.", |_| {});
    remember(&e, "Priya leads the platform team.", |i| {
        i.entities = vec![entity("Priya")];
    });
    let out = e
        .recall(&RecallInput {
            query: "quantum chromodynamics lattice spacing".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(out.memories.is_empty(), "{:?}", out.memories);
    assert!(out.facts.is_empty());
    assert_eq!(out.candidates, 0);
}

#[test]
fn follow_walks_to_a_named_target() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Noah Tansen's manager is Mira Shah.", |i| {
        i.entities = vec![entity("Noah Tansen"), entity("Mira Shah")];
        i.relations = vec![relation(
            "Noah Tansen",
            "manager",
            "Mira Shah",
            "Noah Tansen's manager is Mira Shah.",
        )];
    });
    remember(&e, "Mira Shah lives in Oslo.", |i| {
        i.entities = vec![entity("Mira Shah"), entity("Oslo")];
        i.relations = vec![relation(
            "Mira Shah",
            "lives in",
            "Oslo",
            "Mira Shah lives in Oslo.",
        )];
    });
    let out = paths::follow(
        &e,
        &paths::FollowInput {
            from: "Noah Tansen".into(),
            to: Some("Oslo".into()),
            depth: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.reached);
    assert!(
        out.facts.iter().any(|f| f.target == "Oslo"),
        "{:?}",
        out.facts
    );
    assert!(out.facts.iter().any(|f| f.target == "Mira Shah"));
}

#[test]
fn follow_prefers_forward_edges_over_hub_noise() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Nimbus Prism is a project.", |i| {
        i.entities = vec![entity("Nimbus Prism")];
    });
    for name in ["Ada", "Bea", "Cleo", "Dora", "Eli"] {
        remember(&e, &format!("{name} prefers Nimbus Prism."), |i| {
            i.entities = vec![entity(name), entity("Nimbus Prism")];
            i.relations = vec![relation(
                name,
                "prefer",
                "Nimbus Prism",
                &format!("{name} prefers Nimbus Prism."),
            )];
        });
    }
    remember(&e, "Yash leads Nimbus Prism.", |i| {
        i.entities = vec![entity("Yash"), entity("Nimbus Prism")];
        i.relations = vec![relation(
            "Yash",
            "lead",
            "Nimbus Prism",
            "Yash leads Nimbus Prism.",
        )];
    });
    remember(&e, "Nimbus Prism uses Scala.", |i| {
        i.entities = vec![entity("Nimbus Prism"), entity("Scala")];
        i.relations = vec![relation(
            "Nimbus Prism",
            "use",
            "Scala",
            "Nimbus Prism uses Scala.",
        )];
    });
    let out = paths::follow(
        &e,
        &paths::FollowInput {
            from: "Yash".into(),
            depth: Some(2),
            k: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        out.facts.iter().any(|f| f.target == "Scala"),
        "hub reverse edges crowded out the forward hop: {:?}",
        out.facts.iter().map(|f| &f.fact).collect::<Vec<_>>()
    );
}

#[test]
fn aggregate_counts_entities_and_separates_negatives() {
    let e = engine(Arc::new(HashEmbedder));
    for name in ["Priya", "Anika"] {
        remember(&e, &format!("{name} works at the Solberg Lab."), |i| {
            i.entities = vec![entity(name), entity("Solberg Lab")];
            i.relations = vec![relation(
                name,
                "works at",
                "Solberg Lab",
                &format!("{name} works at the Solberg Lab."),
            )];
        });
    }
    remember(&e, "Dana does not work at the Solberg Lab.", |i| {
        i.entities = vec![entity("Dana"), entity("Solberg Lab")];
        i.relations = vec![relation(
            "Dana",
            "works at",
            "Solberg Lab",
            "Dana does not work at the Solberg Lab.",
        )];
    });
    let out = paths::aggregate(
        &e,
        &paths::AggregateInput {
            predicate: Some("works at".into()),
            target: Some("Solberg Lab".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out.count, 2, "{:?}", out.items);
    assert!(out.items.iter().any(|item| item.name == "Priya"));
    assert!(out.items.iter().any(|item| item.name == "Anika"));
    assert_eq!(out.negative.len(), 1, "{:?}", out.negative);
    assert_eq!(out.negative[0].name, "Dana");
}

struct MapEmbedder {
    map: HashMap<String, Vec<f32>>,
}

impl Embedder for MapEmbedder {
    fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>> {
        let fallback = HashEmbedder;
        Ok(texts
            .iter()
            .map(|text| match self.map.get(*text) {
                Some(vector) => vector.clone(),
                None => fallback.embed(&[text], mode).unwrap().remove(0),
            })
            .collect())
    }

    fn name(&self) -> &str {
        "map"
    }
}

fn basis(i: usize) -> Vec<f32> {
    let mut v = vec![0.0; DIMS];
    v[i] = 1.0;
    v
}

fn mix(a: f32, b: f32) -> Vec<f32> {
    let norm = (a * a + b * b).sqrt();
    let mut v = vec![0.0; DIMS];
    v[0] = a / norm;
    v[1] = b / norm;
    v
}

#[test]
fn entity_knn_seeds_graph_without_an_exact_name() {
    let mut map = HashMap::new();
    map.insert(
        "Kubernetes (tool): container orchestrator".to_string(),
        basis(0),
    );
    map.insert(
        "The cluster upgrade runs every quarter.".to_string(),
        basis(5),
    );
    // cosine 0.9 with the entity card, and the entity name is not in the query.
    map.insert(
        "container orchestrator upgrade schedule".to_string(),
        mix(0.9, 0.435_889_95),
    );
    let e = engine(Arc::new(MapEmbedder { map }));
    let answer = remember(&e, "The cluster upgrade runs every quarter.", |i| {
        i.entities = vec![EntityIn {
            name: "Kubernetes".into(),
            kind: Some("tool".into()),
            description: Some("container orchestrator".into()),
            ..Default::default()
        }];
    });
    let out = e
        .recall(&RecallInput {
            query: "container orchestrator upgrade schedule".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&answer));
    assert!(has_reason(&out, answer, "graph#"));
}

#[test]
fn relation_fact_seed_activates_both_endpoints() {
    let e = engine(Arc::new(HashEmbedder));
    let ravi_memory = remember(&e, "Ravi approved the budget.", |i| {
        i.entities = vec![entity("Ravi")];
    });
    let acme_memory = remember(&e, "Acme Corp shipped the order.", |i| {
        i.entities = vec![entity("Acme Corp")];
    });
    remember(&e, "Ravi signed the annual supply agreement.", |i| {
        i.entities = vec![entity("Ravi"), entity("Acme Corp")];
        i.relations = vec![relation(
            "Ravi",
            "signed",
            "Acme Corp",
            "Ravi signed the annual supply agreement",
        )];
    });
    let out = e
        .recall(&RecallInput {
            query: "who signed the annual supply agreement".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(ids(&out).contains(&ravi_memory));
    assert!(ids(&out).contains(&acme_memory));
    assert!(has_reason(&out, ravi_memory, "graph#"));
    assert!(has_reason(&out, acme_memory, "graph#"));
}

#[test]
fn temporal_window_ranks_the_in_window_event_first() {
    let e = engine(Arc::new(HashEmbedder));
    let in_window = remember(&e, "We shipped the beta release.", |i| {
        i.valid_from = Some("2026-10-14".into());
    });
    remember(&e, "We planned the beta release.", |i| {
        i.valid_from = Some("2026-09-01".into());
    });
    let out = e
        .recall(&RecallInput {
            query: "beta release shipped last week".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.memories[0].id, in_window);
    assert!(has_reason(&out, in_window, "time"));
}

#[test]
fn budget_packing_reports_omitted_memories() {
    let e = engine(Arc::new(HashEmbedder));
    for i in 0..5 {
        remember(
            &e,
            &format!(
                "quarterly compliance audit checklist item {i} {}",
                "detail ".repeat(60)
            ),
            |_| {},
        );
    }
    let out = e
        .recall(&RecallInput {
            query: "quarterly compliance audit checklist".into(),
            k: Some(5),
            budget: Some(60),
            ..Default::default()
        })
        .unwrap();
    assert!(out.omitted >= 1);
    assert!(out.memories.len() < out.candidates);
}

#[test]
fn facts_include_current_and_ended_with_history() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Ravi lives in Delhi.", |i| {
        i.entities = vec![entity("Ravi")];
        i.relations = vec![RelationIn {
            valid_from: Some("2025-01-01".into()),
            valid_to: Some("2026-01-01".into()),
            ..relation("Ravi", "lives in", "Delhi", "Ravi lives in Delhi")
        }];
    });
    remember(&e, "Ravi lives in Pune.", |i| {
        i.entities = vec![entity("Ravi")];
        i.relations = vec![RelationIn {
            valid_from: Some("2026-01-01".into()),
            ..relation("Ravi", "lives in", "Pune", "Ravi lives in Pune")
        }];
    });
    let out = e
        .recall(&RecallInput {
            query: "where does Ravi live".into(),
            include_history: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.facts.len(), 2);
    assert!(out.facts.iter().any(|f| f.ended));
    assert!(out.facts.iter().any(|f| !f.ended));
}

#[test]
fn entity_profile_has_card_facts_and_timeline() {
    let e = engine(Arc::new(HashEmbedder));
    remember(&e, "Priya reviews the roadmap.", |i| {
        i.entities = vec![EntityIn {
            name: "Priya".into(),
            kind: Some("person".into()),
            description: Some("the product lead".into()),
            aliases: vec!["Priya S".into()],
            ..Default::default()
        }];
    });
    remember(&e, "Priya mentors Anika.", |i| {
        i.entities = vec![entity("Priya"), entity("Anika")];
        i.relations = vec![relation("Priya", "mentors", "Anika", "Priya mentors Anika")];
    });
    let out = e.entity("Priya", 10).unwrap();
    assert_eq!(out.card.name, "Priya");
    assert_eq!(out.card.kind, "person");
    assert!(out.card.aliases.contains(&"priya s".to_string()));
    assert_eq!(out.timeline.len(), 2);
    assert!(!out.current_facts.is_empty());
}

#[test]
fn overview_finds_two_communities() {
    let e = engine(Arc::new(HashEmbedder));
    for (source, target) in [("Alpha", "Beta"), ("Beta", "Gamma"), ("Gamma", "Alpha")] {
        remember(&e, &format!("{source} works with {target}."), |i| {
            i.entities = vec![
                EntityIn {
                    name: source.into(),
                    kind: Some("project".into()),
                    ..Default::default()
                },
                EntityIn {
                    name: target.into(),
                    kind: Some("project".into()),
                    ..Default::default()
                },
            ];
            i.relations = vec![relation(
                source,
                "works with",
                target,
                &format!("{source} works with {target}"),
            )];
        });
    }
    for (source, target) in [("Delta", "Epsilon"), ("Epsilon", "Zeta"), ("Zeta", "Delta")] {
        remember(&e, &format!("{source} works with {target}."), |i| {
            i.entities = vec![entity(source), entity(target)];
            i.relations = vec![relation(
                source,
                "works with",
                target,
                &format!("{source} works with {target}"),
            )];
        });
    }
    let out = e.overview(None, 10).unwrap();
    assert!(out.communities.len() >= 2);
    assert!(out.communities.iter().map(|c| c.size).sum::<usize>() >= 6);
    assert!(out.communities.iter().all(|c| c.representative.is_some()));
}

#[test]
fn overview_drops_junk_and_chooses_typed_labels_and_representative() {
    let e = engine(Arc::new(HashEmbedder));
    let representative = remember(&e, "Alpha Beta Alpha roadmap decision with details.", |i| {
        i.entities = vec![
            EntityIn {
                name: "Alpha".into(),
                kind: Some("project".into()),
                ..Default::default()
            },
            EntityIn {
                name: "Beta".into(),
                kind: Some("project".into()),
                ..Default::default()
            },
        ];
        i.relations = vec![relation(
            "Alpha",
            "works with",
            "Beta",
            "Alpha works with Beta",
        )];
    });
    remember(&e, "Alpha release planning.", |i| {
        i.entities = vec![EntityIn {
            name: "Alpha".into(),
            kind: Some("project".into()),
            ..Default::default()
        }];
    });
    remember(&e, "junk only.", |i| {
        i.entities = vec![entity("junk")];
    });
    let out = e.overview(None, 10).unwrap();
    assert_eq!(out.communities.len(), 1);
    let community = &out.communities[0];
    assert_eq!(community.entities[0].name, "Alpha");
    assert_eq!(
        community.representative.as_ref().unwrap().id,
        representative
    );
    assert!(community.representative.as_ref().unwrap().content.len() < 161);
}

#[test]
fn recall_bumps_access_count() {
    let e = engine(Arc::new(HashEmbedder));
    let id = remember(&e, "The backup runs nightly.", |_| {});
    let before: i64 =
        e.db.sql()
            .query_row("SELECT access_count FROM memories WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
    e.recall(&RecallInput {
        query: "backup nightly".into(),
        ..Default::default()
    })
    .unwrap();
    let (after, last): (i64, Option<i64>) =
        e.db.sql()
            .query_row(
                "SELECT access_count,last_access FROM memories WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(after, before + 1);
    assert_eq!(last, Some(clock().now));
}

#[test]
fn sweep_ingests_once_and_reports_each_variant() {
    let path = std::env::temp_dir().join(format!(
        "neocortex-eval-{}-{}.json",
        std::process::id(),
        "sweep"
    ));
    std::fs::write(
        &path,
        r#"{
            "now": "2026-10-21T15:00:00Z",
            "memories": [
                {"key":"a","content":"The office is in Pune.","kind":"fact","tags":[],"importance":0.5,"valid_from":"2026-01-01"},
                {"key":"b","content":"The backup runs nightly.","kind":"fact","tags":[],"importance":0.5,"valid_from":"2026-01-01"}
            ],
            "queries": [{"q":"office","category":"keyword","expect":["a"]}]
        }"#,
    )
    .unwrap();
    let base = Config::default();
    let variants = vec![
        ("one".to_string(), base.clone()),
        ("two".to_string(), base.clone()),
    ];
    let rows = neocortex::eval::sweep(&path, Arc::new(HashEmbedder), base, variants).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(rows.len(), 2);
    for (_, report) in &rows {
        assert_eq!(report.ingested, 2, "fixture must be ingested exactly once");
        assert_eq!(report.queries, 1);
    }
}

#[test]
fn entity_lookup_falls_back_to_prefix_and_suggests_closest() {
    let engine = engine(Arc::new(HashEmbedder));
    remember(&engine, "The lab runs UltraBERT-Embed-150M on CPU.", |m| {
        m.entities = vec![entity("UltraBERT-Embed-150M")];
    });
    let out = engine.entity("UltraBERT", 5).unwrap();
    assert_eq!(out.card.name, "UltraBERT-Embed-150M");
    let err = engine.entity("zzqx unknown", 5).unwrap_err().to_string();
    assert!(
        err.contains("unknown entity") && err.contains("closest:"),
        "{err}"
    );
}
