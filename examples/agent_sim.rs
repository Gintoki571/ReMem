//! Simulates an agent answering a world fixture with two to three tool calls.
//!
//! ```text
//! cargo run --release --example agent_sim -- eval/world-300.json target/w300.db
//! ```
//!
//! Step 1: `recall` the question. Step 2: when the answer is missing, pull the
//! named entity out of the question and `follow` it three hops. Step 3: when it
//! is still missing, `recall` again with the leaf entity name prepended.
//! Accuracy is reported after each step and the context is scored exactly like
//! `neocortex eval` acc@10 (memories + facts + entity cards, lowercased).

use neocortex::engine::{Engine, RecallInput, RecallOutput};
use neocortex::paths::{self, FollowInput, FollowOutput};
use neocortex::temporal::{self, Clock};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

const ACC_K: usize = 10;

fn main() -> neocortex::Result<()> {
    let mut args = std::env::args().skip(1);
    let fixture = PathBuf::from(args.next().unwrap_or_else(|| {
        eprintln!("usage: agent_sim <world.json> <db> [--limit N] [--depth N]");
        std::process::exit(2)
    }));
    let db = PathBuf::from(args.next().unwrap_or_else(|| {
        eprintln!("usage: agent_sim <world.json> <db> [--limit N] [--depth N]");
        std::process::exit(2)
    }));
    let mut limit = usize::MAX;
    let mut depth = 3;
    while let Some(flag) = args.next() {
        let value = args.next().expect("flag needs a value");
        match flag.as_str() {
            "--limit" => {
                limit = value
                    .parse()
                    .map_err(|e| neocortex::Error(format!("bad --limit: {e}")))?
            }
            "--depth" => {
                depth = value
                    .parse()
                    .map_err(|e| neocortex::Error(format!("bad --depth: {e}")))?
            }
            other => return Err(neocortex::Error(format!("unknown flag {other}"))),
        }
    }

    let data: Value = serde_json::from_str(&std::fs::read_to_string(&fixture)?)?;
    let now = temporal::parse_instant(
        data["now"].as_str().unwrap_or_default(),
        &Clock {
            now: 0,
            tz_offset_secs: 0,
        },
    )
    .ok_or_else(|| neocortex::Error("fixture has no valid `now`".to_string()))?;
    let clock = Clock {
        now,
        tz_offset_secs: 0,
    };
    let engine =
        Engine::open(&db, Arc::new(neocortex::mcp::LazyEmbedder::new()))?.with_clock(clock);

    let mut total = 0usize;
    let mut answered = 0usize;
    let mut solved = [0usize; 4];
    let mut calls = 0usize;
    let mut recall_ms: Vec<f64> = Vec::new();
    let mut follow_ms: Vec<f64> = Vec::new();
    let mut by_category: HashMap<String, (usize, usize)> = HashMap::new();

    for query in data["queries"].as_array().into_iter().flatten() {
        if total >= limit {
            break;
        }
        let answers: Vec<String> = query["answers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str().map(str::to_lowercase))
            .collect();
        if answers.is_empty() {
            continue;
        }
        total += 1;
        let q = query["q"].as_str().unwrap_or_default();
        let category = query["category"].as_str().unwrap_or("other").to_string();
        let mut used = 0usize;

        let started = Instant::now();
        let first = engine.recall(&RecallInput {
            query: q.to_string(),
            k: Some(ACC_K),
            as_of: query["as_of"].as_str().map(str::to_string),
            include_history: query["include_history"].as_bool().unwrap_or(false),
            ..Default::default()
        })?;
        recall_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        used += 1;
        let mut step = if holds(&context(&first), &answers) {
            1
        } else {
            0
        };

        if step == 0 {
            if let Some(from) = question_entity(&engine, q)? {
                let started = Instant::now();
                let out = paths::follow(
                    &engine,
                    &FollowInput {
                        from,
                        depth: Some(depth),
                        k: Some(40),
                        ..Default::default()
                    },
                )?;
                follow_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                used += 1;
                if holds(&follow_context(&out, depth), &answers) {
                    step = 2;
                } else if let Some(leaf) = leaf_entity(&out) {
                    let started = Instant::now();
                    let again = engine.recall(&RecallInput {
                        query: format!("{leaf} {q}"),
                        k: Some(ACC_K),
                        as_of: query["as_of"].as_str().map(str::to_string),
                        include_history: query["include_history"].as_bool().unwrap_or(false),
                        ..Default::default()
                    })?;
                    recall_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                    used += 1;
                    if holds(&context(&again), &answers) {
                        step = 3;
                    }
                }
            }
        }

        calls += used;
        if step > 0 {
            answered += 1;
            solved[step] += 1;
        }
        let entry = by_category.entry(category).or_default();
        entry.0 += 1;
        if step > 0 {
            entry.1 += 1;
        }
    }

    println!(
        "\nqueries {total}, answered {answered} ({:.3})",
        rate(answered, total)
    );
    println!(
        "  step 1 recall {:.3} | +follow {:.3} | +recall-leaf {:.3}",
        rate(solved[1], total),
        rate(solved[1] + solved[2], total),
        rate(answered, total)
    );
    println!(
        "  solved by: recall {} follow {} recall-leaf {} missed {}",
        solved[1],
        solved[2],
        solved[3],
        total - answered
    );
    println!(
        "  calls/query {:.2}, recall p50 {:.0} ms, follow p50 {} ms",
        calls as f64 / total.max(1) as f64,
        p50(&mut recall_ms),
        if follow_ms.is_empty() {
            "-".to_string()
        } else {
            format!("{:.0}", p50(&mut follow_ms))
        }
    );
    let mut rows: Vec<_> = by_category.into_iter().collect();
    rows.sort();
    for (category, (n, hit)) in rows {
        println!("  {category:<18} {hit:>3}/{n:<3} {:.3}", rate(hit, n));
    }
    Ok(())
}

fn p50(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn rate(hit: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        hit as f64 / total as f64
    }
}

fn holds(text: &str, answers: &[String]) -> bool {
    answers.iter().all(|answer| text.contains(answer))
}

/// Same view `neocortex eval` scores: 10 memories, facts, entity cards.
fn context(out: &RecallOutput) -> String {
    let mut text = String::new();
    for memory in out.memories.iter().take(10) {
        text.push_str(&memory.content);
        text.push('\n');
    }
    for fact in &out.facts {
        text.push_str(&fact.fact);
        text.push('\n');
        text.push_str(&fact.source);
        text.push(' ');
        text.push_str(&fact.target);
        text.push('\n');
    }
    for entity in &out.entities {
        text.push_str(&entity.name);
        text.push(' ');
        text.push_str(&entity.aliases.join(" "));
        text.push(' ');
        text.push_str(&entity.description);
        text.push('\n');
    }
    text.to_lowercase()
}

fn follow_context(out: &FollowOutput, depth: usize) -> String {
    let mut text = String::new();
    for fact in out.facts.iter().filter(|fact| fact.hops <= depth) {
        text.push_str(&fact.fact);
        text.push('\n');
        text.push_str(&fact.source);
        text.push(' ');
        text.push_str(&fact.target);
        text.push('\n');
    }
    text.to_lowercase()
}

/// The deepest fact the walk reached; its far end is where the answer lives.
fn leaf_entity(out: &FollowOutput) -> Option<String> {
    let deepest = out.facts.iter().map(|fact| fact.hops).max()?;
    let fact = out.facts.iter().find(|fact| fact.hops == deepest)?;
    Some(if fact.direction == "out" {
        fact.target.clone()
    } else {
        fact.source.clone()
    })
}

/// Longest word run in the question that names an entity exactly.
fn question_entity(engine: &Engine, question: &str) -> neocortex::Result<Option<String>> {
    let words: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches("'s").to_string())
        .collect();
    for len in (1..=3).rev() {
        for start in 0..=words.len().saturating_sub(len) {
            let candidate = words[start..start + len].join(" ");
            let name: Option<String> = engine
                .db
                .sql()
                .query_row(
                    "SELECT e.name FROM entity_aliases a
                       JOIN entities e ON e.id = a.entity_id
                      WHERE a.alias_norm = ?1 LIMIT 1",
                    [neocortex::text::norm_name(&candidate)],
                    |row| row.get(0),
                )
                .ok();
            if name.is_some() {
                return Ok(name);
            }
        }
    }
    Ok(None)
}
