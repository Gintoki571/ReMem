use neocortex::extract;
use neocortex::extract_model::extract_text;
use neocortex::temporal::Clock;
use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::time::Instant;

#[derive(Deserialize)]
struct Gold {
    memories: Vec<Memory>,
}

#[derive(Deserialize)]
struct Memory {
    #[serde(default)]
    content: String,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    relations: Vec<Relation>,
    #[serde(default)]
    valid_from: Option<String>,
}

#[derive(Deserialize)]
struct Entity {
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct Relation {
    source: String,
    predicate: String,
    target: String,
    #[serde(default)]
    negated: bool,
}

fn main() {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "eval/extract/gold.json".to_string());
    let gold: Gold =
        serde_json::from_str(&fs::read_to_string(path).expect("read extraction gold")).unwrap();
    // Optional second argument: predictions JSON aligned with gold.memories
    // (same shape as gold memories), used to score external systems.
    let external: Option<Vec<Memory>> = env::args().nth(2).map(|path| {
        serde_json::from_str(&fs::read_to_string(path).expect("read predictions"))
            .expect("parse predictions")
    });
    let baseline = score(&gold, |_, text| {
        let entities = extract::heuristic_entities(text)
            .into_iter()
            .map(|candidate| Entity {
                name: candidate.name,
                kind: String::new(),
            })
            .collect();
        (entities, Vec::new(), None)
    });
    let rules = score(&gold, |_, text| {
        let result = extract_text(text, &Clock::system());
        (
            result
                .entities
                .into_iter()
                .map(|entity| Entity {
                    name: entity.name,
                    kind: entity.kind.unwrap_or_default(),
                })
                .collect(),
            result
                .relations
                .into_iter()
                .map(|relation| Relation {
                    source: relation.source,
                    predicate: relation.predicate,
                    target: relation.target,
                    negated: relation.negated.unwrap_or(false),
                })
                .collect(),
            result.valid_from,
        )
    });
    println!("system,entity_p,entity_r,entity_f1,relation_p,relation_r,relation_f1,date_accuracy,negation_accuracy,median_us");
    println!(
        "heuristic,{},{},{},{},{},{},{},{},{}",
        baseline.0,
        baseline.1,
        baseline.2,
        baseline.3,
        baseline.4,
        baseline.5,
        baseline.6,
        baseline.7,
        baseline.8
    );
    println!(
        "rules,{},{},{},{},{},{},{},{},{}",
        rules.0, rules.1, rules.2, rules.3, rules.4, rules.5, rules.6, rules.7, rules.8
    );
    if let Some(predictions) = external {
        let name = env::args().nth(3).unwrap_or_else(|| "external".to_string());
        let row = score(&gold, |index, _| {
            let memory = &predictions[index];
            (
                memory
                    .entities
                    .iter()
                    .map(|e| Entity {
                        name: e.name.clone(),
                        kind: e.kind.clone(),
                    })
                    .collect(),
                memory
                    .relations
                    .iter()
                    .map(|r| Relation {
                        source: r.source.clone(),
                        predicate: r.predicate.clone(),
                        target: r.target.clone(),
                        negated: r.negated,
                    })
                    .collect(),
                memory.valid_from.clone(),
            )
        });
        println!(
            "{name},{},{},{},{},{},{},{},{},{}",
            row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8
        );
    }
}

#[allow(clippy::type_complexity)]
fn score<F>(
    gold: &Gold,
    mut run: F,
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
)
where
    F: FnMut(usize, &str) -> (Vec<Entity>, Vec<Relation>, Option<String>),
{
    let mut entity_tp = 0;
    let mut entity_pred = 0;
    let mut entity_gold = 0;
    let mut relation_tp = 0;
    let mut relation_pred = 0;
    let mut relation_gold = 0;
    let mut date_ok = 0;
    let mut date_total = 0;
    let mut neg_ok = 0;
    let mut neg_total = 0;
    let start = Instant::now();
    for (index, memory) in gold.memories.iter().enumerate() {
        let (entities, relations, date) = run(index, &memory.content);
        let expected_entities: HashSet<_> = memory
            .entities
            .iter()
            .map(|entity| entity.name.to_lowercase())
            .collect();
        let predicted_entities: HashSet<_> = entities
            .iter()
            .map(|entity| entity.name.to_lowercase())
            .collect();
        entity_tp += expected_entities.intersection(&predicted_entities).count();
        entity_pred += predicted_entities.len();
        entity_gold += expected_entities.len();
        let expected_relations: HashSet<_> = memory
            .relations
            .iter()
            .map(|relation| {
                (
                    relation.source.to_lowercase(),
                    canonical(&relation.predicate),
                    relation.target.to_lowercase(),
                )
            })
            .collect();
        let predicted_relations: HashSet<_> = relations
            .iter()
            .map(|relation| {
                (
                    relation.source.to_lowercase(),
                    canonical(&relation.predicate),
                    relation.target.to_lowercase(),
                )
            })
            .collect();
        relation_tp += expected_relations
            .intersection(&predicted_relations)
            .count();
        relation_pred += predicted_relations.len();
        relation_gold += expected_relations.len();
        if let Some(expected) = &memory.valid_from {
            date_total += 1;
            if date.as_deref() == Some(expected) {
                date_ok += 1;
            }
        }
        for expected in &memory.relations {
            if let Some(found) = relations.iter().find(|relation| {
                relation.source.eq_ignore_ascii_case(&expected.source)
                    && relation.target.eq_ignore_ascii_case(&expected.target)
                    && canonical(&relation.predicate) == canonical(&expected.predicate)
            }) {
                neg_total += 1;
                neg_ok += usize::from(found.negated == expected.negated);
            }
        }
    }
    let elapsed = start.elapsed().as_micros() / gold.memories.len() as u128;
    (
        metric(entity_tp, entity_pred),
        metric(entity_tp, entity_gold),
        f1(entity_tp, entity_pred, entity_gold),
        metric(relation_tp, relation_pred),
        metric(relation_tp, relation_gold),
        f1(relation_tp, relation_pred, relation_gold),
        metric(date_ok, date_total),
        metric(neg_ok, neg_total),
        elapsed.to_string(),
    )
}

fn canonical(predicate: &str) -> String {
    let p = predicate.to_lowercase();
    if p.contains("live") || p.contains("reside") {
        "live".into()
    } else if p.contains("work") || p.contains("employ") {
        "work".into()
    } else if p.contains("prefer") {
        "prefer".into()
    } else if p.contains("report") {
        "report".into()
    } else {
        p.split_whitespace().next().unwrap_or("").into()
    }
}

fn metric(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        "1.000".into()
    } else {
        format!("{:.3}", numerator as f64 / denominator as f64)
    }
}

fn f1(tp: usize, predicted: usize, expected: usize) -> String {
    let p = tp as f64 / predicted.max(1) as f64;
    let r = tp as f64 / expected.max(1) as f64;
    format!(
        "{:.3}",
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    )
}
