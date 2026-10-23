//! `neocortex bench` (design §10): a deterministic synthetic load, then
//! remember/recall latency percentiles, database size and peak RSS.
//!
//! The corpus is generated from a fixed xorshift seed so two runs on the same
//! build measure the same work. Defaults target the design budget (10k
//! memories, 200 queries, warm process, CPU embedder).

use crate::embed::{Embedder, Mode};
use crate::engine::{Engine, EntityIn, RecallInput, RelationIn, RememberInput};
use crate::error::Result;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

const DAY: i64 = 86_400;
const YEARS: i64 = 3;
const BATCH: usize = 64;
const SEED: u64 = 0x2545_f491_4f6c_dd1d;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub memories: usize,
    pub queries: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            memories: 10_000,
            queries: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub memories: usize,
    pub queries: usize,
    pub embedder: String,
    pub remember_p50_ms: f64,
    pub remember_p95_ms: f64,
    pub remember_per_sec: f64,
    pub recall_p50_ms: f64,
    pub recall_p95_ms: f64,
    pub db_bytes: u64,
    pub peak_rss_mb: f64,
}

impl BenchReport {
    pub fn render(&self) -> String {
        format!(
            "bench: {} memories, {} queries, embedder {}\n\
             \x20 remember   p50 {:>8.2} ms   p95 {:>8.2} ms   {:>8.0} memories/s\n\
             \x20 recall     p50 {:>8.2} ms   p95 {:>8.2} ms\n\
             \x20 database   {:>8.1} MB\n\
             \x20 peak RSS   {:>8.1} MB\n",
            self.memories,
            self.queries,
            self.embedder,
            self.remember_p50_ms,
            self.remember_p95_ms,
            self.remember_per_sec,
            self.recall_p50_ms,
            self.recall_p95_ms,
            self.db_bytes as f64 / 1_048_576.0,
            self.peak_rss_mb
        )
    }
}

/// Run the benchmark against an empty database at `path`.
pub fn run(path: &Path, emb: Arc<dyn Embedder>, opts: Options) -> Result<BenchReport> {
    if opts.memories == 0 {
        return Err(crate::err!("bench needs at least one memory"));
    }
    let engine = Engine::open(path, Arc::clone(&emb))?;
    let existing = engine.stats()?.memories;
    if existing > 0 {
        return Err(crate::err!(
            "bench needs an empty database; {path:?} already holds {existing} memories"
        ));
    }
    let now = engine.clock().now;
    let mut rng = Rng(SEED);
    let world = World::new();
    let inputs = corpus(&world, &mut rng, opts.memories, now);

    // Load the model (and warm the kernels) outside the timed region.
    emb.embed(&["warm up"], Mode::Document)?;

    let mut per_memory: Vec<f64> = Vec::with_capacity(inputs.len().div_ceil(BATCH));
    let started = Instant::now();
    for chunk in inputs.chunks(BATCH) {
        let begin = Instant::now();
        for input in chunk {
            engine.remember(input.clone())?;
        }
        per_memory.push(begin.elapsed().as_secs_f64() * 1000.0 / chunk.len() as f64);
    }
    let elapsed = started.elapsed().as_secs_f64();

    let queries = query_corpus(&world, &mut rng, opts.queries, now);
    let mut recall_ms: Vec<f64> = Vec::with_capacity(queries.len());
    for query in &queries {
        let begin = Instant::now();
        engine.recall(&RecallInput {
            query: query.clone(),
            k: Some(10),
            ..RecallInput::default()
        })?;
        recall_ms.push(begin.elapsed().as_secs_f64() * 1000.0);
    }

    let _ = engine
        .db
        .sql()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
    let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
    let db_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        + std::fs::metadata(wal).map_or(0, |m| m.len());

    Ok(BenchReport {
        memories: inputs.len(),
        queries: queries.len(),
        embedder: emb.name().into(),
        remember_p50_ms: percentile(&mut per_memory.clone(), 0.50),
        remember_p95_ms: percentile(&mut per_memory.clone(), 0.95),
        remember_per_sec: inputs.len() as f64 / elapsed,
        recall_p50_ms: percentile(&mut recall_ms.clone(), 0.50),
        recall_p95_ms: percentile(&mut recall_ms, 0.95),
        db_bytes,
        peak_rss_mb: peak_rss_kb() as f64 / 1024.0,
    })
}

/// Nearest-rank percentile over an unsorted sample.
pub fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let rank = (values.len() as f64 * p).ceil() as usize;
    values[rank.clamp(1, values.len()) - 1]
}

/// VmHWM from /proc/self/status, in kB; 0 where /proc is unavailable.
fn peak_rss_kb() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- corpus

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// ~300 fictional names: 100 people, 100 places, 50 projects, 50 tools.
struct World {
    people: Vec<String>,
    places: Vec<String>,
    projects: Vec<String>,
    tools: Vec<String>,
    months: Vec<&'static str>,
    weekdays: Vec<&'static str>,
}

impl World {
    fn new() -> World {
        const FIRST: [&str; 20] = [
            "Ada", "Bram", "Cleo", "Dara", "Emil", "Faye", "Goran", "Hana", "Ivo", "Juno", "Kai",
            "Lena", "Milo", "Nadia", "Oren", "Pia", "Quinn", "Rhea", "Soren", "Tara",
        ];
        const LAST: [&str; 30] = [
            "Alder", "Bexley", "Corvin", "Dunmore", "Ellery", "Fenwick", "Gale", "Hollis",
            "Ibarra", "Jarrow", "Kestrel", "Lorne", "Merrick", "Noor", "Oakes", "Pryce", "Quill",
            "Rask", "Sable", "Thorne", "Ulm", "Vance", "Wren", "Xanth", "Yarrow", "Zeller",
            "Ashby", "Brill", "Cade", "Dove",
        ];
        const PLACE_A: [&str; 20] = [
            "Ald", "Brack", "Cinder", "Dun", "Ember", "Frost", "Glim", "Harrow", "Iron", "Juniper",
            "Kelp", "Lantern", "Moss", "North", "Oak", "Pine", "Quarry", "Rook", "Salt", "Thorn",
        ];
        const PLACE_B: [&str; 5] = ["ford", "haven", "reach", "vale", "wick"];
        const PROJECT_A: [&str; 10] = [
            "Amber", "Brisk", "Cobalt", "Dusky", "Ember", "Fleet", "Golden", "Hollow", "Ivory",
            "Jade",
        ];
        const PROJECT_B: [&str; 5] = ["Atlas", "Beacon", "Cipher", "Drift", "Echo"];
        const TOOL_A: [&str; 10] = [
            "quartz", "rivet", "slate", "tinder", "umber", "vault", "willow", "xenon", "yarrow",
            "zephyr",
        ];
        const TOOL_B: [&str; 5] = ["ctl", "dash", "forge", "kit", "sync"];

        World {
            people: (0..100)
                .map(|i| format!("{} {}", FIRST[i % 20], LAST[(i / 20) % 30]))
                .collect(),
            places: PLACE_A
                .iter()
                .flat_map(|a| PLACE_B.iter().map(move |b| format!("{a}{b}")))
                .collect(),
            projects: PROJECT_A
                .iter()
                .flat_map(|a| PROJECT_B.iter().map(move |b| format!("Project {a}{b}")))
                .collect(),
            tools: TOOL_A
                .iter()
                .flat_map(|a| TOOL_B.iter().map(move |b| format!("{a}{b}")))
                .collect(),
            months: vec![
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            weekdays: vec![
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
        }
    }

    fn person(&self, rng: &mut Rng) -> &str {
        rng.pick(&self.people)
    }

    fn place(&self, rng: &mut Rng) -> &str {
        rng.pick(&self.places)
    }

    fn project(&self, rng: &mut Rng) -> &str {
        rng.pick(&self.projects)
    }

    fn tool(&self, rng: &mut Rng) -> &str {
        rng.pick(&self.tools)
    }
}

fn iso(ts: i64) -> String {
    crate::export::fmt_iso(ts)
}

/// (month index 0..11, year) in UTC.
fn month_year(ts: i64) -> (usize, i64) {
    let (y, m, _) = crate::temporal::civil_from_days(ts.div_euclid(DAY));
    ((m - 1) as usize, y)
}

/// A person/project/tool/place card.
fn entity(name: &str, kind: &str) -> EntityIn {
    EntityIn {
        name: name.into(),
        kind: Some(kind.into()),
        description: Some(format!("synthetic {kind} used by the benchmark")),
        aliases: Vec::new(),
        same_as: None,
    }
}

fn relation(source: &str, predicate: &str, target: &str, fact: String, from: i64) -> RelationIn {
    RelationIn {
        source: source.into(),
        predicate: predicate.into(),
        target: target.into(),
        fact: Some(fact),
        exclusive: Some(false),
        negated: None,
        valid_from: Some(iso(from)),
        valid_to: None,
    }
}

/// N varied memories: 8 templates, 20% carry structured entities/relations,
/// dates spread over three years ending at `now`.
fn corpus(world: &World, rng: &mut Rng, n: usize, now: i64) -> Vec<RememberInput> {
    let span = YEARS * 365 * DAY;
    let base = now - span;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let when = base + rng.below(span as usize) as i64;
        let (month, year) = month_year(when);
        let month = world.months[month];
        let date = crate::temporal::fmt_date(
            when,
            &crate::temporal::Clock {
                now,
                tz_offset_secs: 0,
            },
        );
        let structured = rng.below(5) == 0;
        let template = rng.below(8);
        let (content, kind, entities, relations) = match template {
            0 => {
                let (person, project) = (world.person(rng), world.project(rng));
                let content = format!("{person} reviewed {project} on {date}.");
                let rel = relation(
                    person,
                    "reviewed",
                    project,
                    format!("{person} reviewed {project}"),
                    when,
                );
                (
                    content,
                    "observation",
                    vec![entity(person, "person"), entity(project, "project")],
                    vec![rel],
                )
            }
            1 => {
                let (person, tool, tool2, project) = (
                    world.person(rng),
                    world.tool(rng),
                    world.tool(rng),
                    world.project(rng),
                );
                let content = format!("{person} prefers {tool} over {tool2} for {project}.");
                let rel = relation(
                    person,
                    "prefers",
                    tool,
                    format!("{person} prefers {tool}"),
                    when,
                );
                (
                    content,
                    "preference",
                    vec![
                        entity(person, "person"),
                        entity(tool, "tool"),
                        entity(tool2, "tool"),
                        entity(project, "project"),
                    ],
                    vec![rel],
                )
            }
            2 => {
                let (project, tool, tool2) = (world.project(rng), world.tool(rng), world.tool(rng));
                let content =
                    format!("{project} migrated from {tool} to {tool2} in {month} {year}.");
                let rel = relation(
                    project,
                    "migrated to",
                    tool2,
                    format!("{project} migrated to {tool2}"),
                    when,
                );
                (
                    content,
                    "event",
                    vec![
                        entity(project, "project"),
                        entity(tool, "tool"),
                        entity(tool2, "tool"),
                    ],
                    vec![rel],
                )
            }
            3 => {
                let (person, place) = (world.person(rng), world.place(rng));
                let content = format!("{person} lives in {place}.");
                let mut rel = relation(
                    person,
                    "lives in",
                    place,
                    format!("{person} lives in {place}"),
                    when,
                );
                rel.exclusive = Some(true);
                (
                    content,
                    "fact",
                    vec![entity(person, "person"), entity(place, "place")],
                    vec![rel],
                )
            }
            4 => {
                let (project, place, weekday) = (
                    world.project(rng),
                    world.place(rng),
                    *rng.pick(&world.weekdays),
                );
                let content = format!("{project} deploys from {place} every {weekday}.");
                let rel = relation(
                    project,
                    "deploys from",
                    place,
                    format!("{project} deploys from {place}"),
                    when,
                );
                (
                    content,
                    "fact",
                    vec![entity(project, "project"), entity(place, "place")],
                    vec![rel],
                )
            }
            5 => {
                let (person, tool, project) =
                    (world.person(rng), world.tool(rng), world.project(rng));
                let content =
                    format!("{person} reported a bug in {tool} while working on {project}.");
                let rel = relation(
                    person,
                    "reported bug in",
                    tool,
                    format!("{person} reported a bug in {tool}"),
                    when,
                );
                (
                    content,
                    "mistake",
                    vec![
                        entity(person, "person"),
                        entity(tool, "tool"),
                        entity(project, "project"),
                    ],
                    vec![rel],
                )
            }
            6 => {
                let (tool, tool2, project) = (world.tool(rng), world.tool(rng), world.project(rng));
                let content = format!("{tool} replaced {tool2} in {project} on {date}.");
                let rel = relation(
                    tool,
                    "replaced",
                    tool2,
                    format!("{tool} replaced {tool2}"),
                    when,
                );
                (
                    content,
                    "decision",
                    vec![
                        entity(tool, "tool"),
                        entity(tool2, "tool"),
                        entity(project, "project"),
                    ],
                    vec![rel],
                )
            }
            _ => {
                let (person, person2, place, project) = (
                    world.person(rng),
                    world.person(rng),
                    world.place(rng),
                    world.project(rng),
                );
                let content = format!("{person} met {person2} in {place} to discuss {project}.");
                let rel = relation(
                    person,
                    "met",
                    person2,
                    format!("{person} met {person2}"),
                    when,
                );
                (
                    content,
                    "event",
                    vec![
                        entity(person, "person"),
                        entity(person2, "person"),
                        entity(place, "place"),
                        entity(project, "project"),
                    ],
                    vec![rel],
                )
            }
        };
        out.push(RememberInput {
            content,
            kind: Some(kind.into()),
            tags: vec![kind.into(), "bench".into()],
            importance: Some(0.3 + (rng.below(70) as f32) / 100.0),
            valid_from: Some(iso(when)),
            entities: if structured { entities } else { Vec::new() },
            relations: if structured { relations } else { Vec::new() },
            ..RememberInput::default()
        });
    }
    out
}

/// A third keyword phrases, a third entity lookups, a third temporal windows.
fn query_corpus(world: &World, rng: &mut Rng, n: usize, now: i64) -> Vec<String> {
    let span = YEARS * 365 * DAY;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let query = match i % 3 {
            0 => format!(
                "{} {}",
                world.project(rng),
                *rng.pick(&["migrated", "deploys", "reviewed"])
            ),
            1 => format!("{} {}", world.person(rng), world.project(rng)),
            _ => {
                let when = now - span + rng.below(span as usize) as i64;
                let (month, year) = month_year(when);
                format!("what happened in {} {year}", world.months[month])
            }
        };
        out.push(query);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_deterministic_and_varied() {
        let world = World::new();
        let mut a = Rng(SEED);
        let mut b = Rng(SEED);
        let first = corpus(&world, &mut a, 200, 1_800_000_000);
        let second = corpus(&world, &mut b, 200, 1_800_000_000);
        assert_eq!(
            first.iter().map(|m| &m.content).collect::<Vec<_>>(),
            second.iter().map(|m| &m.content).collect::<Vec<_>>()
        );
        assert!(first.iter().filter(|m| !m.relations.is_empty()).count() >= 20);
        let unique: std::collections::HashSet<&String> = first.iter().map(|m| &m.content).collect();
        assert!(
            unique.len() > 150,
            "corpus should not collapse to a few texts"
        );
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let mut values = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&mut values, 0.5), 2.0);
        assert_eq!(percentile(&mut values, 0.95), 4.0);
        assert_eq!(percentile(&mut [], 0.5), 0.0);
    }
}
