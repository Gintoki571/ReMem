//! Query-directed relation-path retrieval and agent-facing graph tools.
//!
//! This is the LLM-free shape of Think-on-Graph / RoG path retrieval: start at
//! the entities the question names, expand only over relations whose predicate
//! or fact text matches question terms, and keep the paths that cover the most
//! distinct question terms. The last hop's evidence memory is the answer, even
//! when that memory never names the entity the question started from.
//!
//! Terms are matched on a small expansion set (`lives` / `live`, plurals,
//! possessives) instead of a porter-style stem, because the same word is often
//! a noun in one fact and a verb in another ("lives in" vs "a life in").

use crate::db::Db;
use crate::engine::{Engine, FactHit};
use crate::text;
use crate::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Paths kept per expansion step.
const BEAM: usize = 24;
/// Longest chain considered (world three-hop queries need 3).
const MAX_DEPTH: usize = 3;
/// Relations taken from one entity, best term match first.
const EDGES_PER_ENTITY: usize = 12;
/// Ranked paths kept per query.
const PATH_HITS: usize = 12;
/// Reverse traversal is allowed but less likely than forward in stored facts.
const REVERSE_PENALTY: f64 = 0.75;
/// Words that name the answer rather than a relation: they must not steer paths.
const ANSWER_WORDS: &[&str] = &[
    "address", "city", "count", "country", "date", "email", "kind", "language", "list", "name",
    "names", "number", "phone", "place", "town", "type", "value",
];

const NEGATION_WORDS: &[&str] = &[
    "arent", "cannot", "cant", "didnt", "doesnt", "dont", "except", "hadnt", "hasnt", "havent",
    "isnt", "neither", "never", "no", "nobody", "none", "nor", "not", "nothing", "wasnt", "werent",
    "without", "wont",
];

/// Which relations a query may traverse: the same bi-temporal rules `recall`
/// applies to its graph channel.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Visibility {
    pub now: i64,
    pub as_of: Option<i64>,
    pub history: bool,
}

impl Visibility {
    pub(crate) fn shows(
        &self,
        valid_from: i64,
        valid_to: Option<i64>,
        expired: Option<i64>,
    ) -> bool {
        let t = self.as_of.unwrap_or(self.now);
        if valid_from > t {
            return false;
        }
        if self.as_of.is_some() {
            return valid_to.is_none_or(|v| t < v);
        }
        if self.history {
            return true;
        }
        valid_to.is_none_or(|v| self.now < v) && expired.is_none()
    }
}

/// True when the text states a negative or retracted fact.
pub fn has_negation_cue(value: &str) -> bool {
    let folded = text::fold(value);
    if folded.contains("no longer") {
        return true;
    }
    folded.split_whitespace().any(|word| {
        let bare: String = word.chars().filter(|c| *c != '\'' && *c != '’').collect();
        NEGATION_WORDS.contains(&bare.as_str())
    })
}

/// True when the question asks for what is not the case.
pub fn negation_intent(query: &str) -> bool {
    has_negation_cue(query)
}

/// Every surface form a word may take in a stored fact.
fn token_forms(token: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut push = |w: &str| {
        if w.chars().count() > 1 {
            out.insert(w.to_string());
        }
    };
    push(token);
    let base = token.strip_suffix("'s").unwrap_or(token);
    push(base);
    push(&text::singularize(base));
    let verb = text::norm_predicate(base);
    if !verb.contains(' ') {
        push(&verb);
    }
    out
}

fn text_forms(value: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for token in text::tokens(value) {
        out.extend(token_forms(&token));
    }
    out
}

/// Question words mapped onto the relation vocabulary. "home" never appears
/// in "Liam Vijappa lives in Denver", so a purely lexical walk would miss the
/// last hop of "the manager of X's sister's home city". Synonyms are added to
/// the surface forms rather than replacing them.
const TERM_SYNONYMS: &[(&str, &[&str])] = &[
    ("home", &["live"]),
    ("lives", &["home"]),
    ("live", &["home"]),
    ("job", &["work"]),
    ("employer", &["work"]),
    ("company", &["work"]),
    ("work", &["company"]),
    ("works", &["company"]),
    ("boss", &["manager"]),
    ("supervisor", &["manager"]),
    ("sister", &["sibling"]),
    ("brother", &["sibling"]),
    ("sibling", &["brother", "sister"]),
    ("language", &["use"]),
    ("stack", &["use"]),
    ("prefer", &["like"]),
    ("prefers", &["like"]),
    ("like", &["prefer"]),
    ("likes", &["prefer"]),
];

/// Question terms in the order they appear, minus question words, answer-type
/// words and the entity names the question anchors on.
fn query_terms(query: &str, anchor_names: &[String]) -> Vec<HashSet<String>> {
    let anchors: HashSet<String> = anchor_names
        .iter()
        .flat_map(|name| text::tokens(name))
        .flat_map(|token| token_forms(&token))
        .collect();
    let mut out: Vec<HashSet<String>> = Vec::new();
    for token in text::tokens(query) {
        let bare: String = token.chars().filter(|c| *c != '\'').collect();
        if token.chars().count() < 2
            || text::is_stopword(&token)
            || ANSWER_WORDS.contains(&bare.as_str())
        {
            continue;
        }
        let mut forms = token_forms(&token);
        if let Some((_, extra)) = TERM_SYNONYMS.iter().find(|(word, _)| *word == token) {
            forms.extend(extra.iter().map(|word| (*word).to_string()));
        }
        if !forms.is_disjoint(&anchors) || out.iter().any(|existing| !existing.is_disjoint(&forms))
        {
            continue;
        }
        out.push(forms);
        if out.len() >= 16 {
            break;
        }
    }
    out
}

fn edge_match(forms: &HashSet<String>, terms: &[HashSet<String>]) -> (u64, usize) {
    let mut mask = 0u64;
    let mut matched = 0usize;
    for (index, term) in terms.iter().enumerate() {
        if !forms.is_disjoint(term) {
            mask |= 1 << index;
            matched += 1;
        }
    }
    (mask, matched)
}

#[derive(Clone, Debug)]
struct Edge {
    relation_id: i64,
    src: i64,
    dst: i64,
    other: i64,
    predicate: String,
    fact: String,
    reverse: bool,
    mask: u64,
    quality: f64,
}

/// One traversed relation. `reverse` means the walk went dst -> src, which is
/// how "Mira Shah is Noah's manager" is followed from Noah.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Step {
    pub relation_id: i64,
    pub src: i64,
    pub dst: i64,
    pub predicate: String,
    pub fact: String,
    pub reverse: bool,
    pub matched: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PathHit {
    pub steps: Vec<Step>,
    /// Distinct question terms this chain covers.
    pub coverage: usize,
    pub score: f64,
}

#[derive(Clone, Debug)]
struct State {
    entity: i64,
    covered: u64,
    score: f64,
    steps: Vec<Step>,
}

fn incident(
    db: &Db,
    entity: i64,
    vis: Visibility,
    terms: &[HashSet<String>],
    cache: &mut HashMap<i64, Vec<Edge>>,
) -> Result<Vec<Edge>> {
    if let Some(edges) = cache.get(&entity) {
        return Ok(edges.clone());
    }
    let mut st = db.sql().prepare_cached(
        "SELECT r.id,r.src,r.dst,p.canonical,r.fact,r.valid_from,r.valid_to,r.expired_at
         FROM relations r JOIN predicates p ON p.id=r.predicate_id
         WHERE r.src=?1 OR r.dst=?2",
    )?;
    let rows = st.query_map(params![entity, entity], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<i64>>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (relation_id, src, dst, predicate, fact, valid_from, valid_to, expired) = row?;
        if src == dst || !vis.shows(valid_from, valid_to, expired) {
            continue;
        }
        let reverse = dst == entity;
        let forms = text_forms(&format!("{predicate} {fact}"));
        let (mask, matched) = edge_match(&forms, terms);
        if mask == 0 {
            continue;
        }
        let quality = (0.4 + 0.6 * matched as f64 / terms.len() as f64)
            * if reverse { REVERSE_PENALTY } else { 1.0 };
        out.push(Edge {
            relation_id,
            src,
            dst,
            other: if reverse { src } else { dst },
            predicate,
            fact,
            reverse,
            mask,
            quality,
        });
    }
    out.sort_by(|a, b| {
        b.mask
            .count_ones()
            .cmp(&a.mask.count_ones())
            .then(b.quality.total_cmp(&a.quality))
            .then(a.relation_id.cmp(&b.relation_id))
    });
    out.truncate(EDGES_PER_ENTITY);
    cache.insert(entity, out.clone());
    Ok(out)
}

/// Ranked paths that start at one of `seeds` and add at least one new question
/// term per hop.
pub(crate) fn search(db: &Db, query: &str, seeds: &[i64], vis: Visibility) -> Result<Vec<PathHit>> {
    if seeds.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for id in seeds {
        if let Ok(name) = db
            .sql()
            .query_row("SELECT name FROM entities WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
        {
            names.push(name);
        }
    }
    let terms = query_terms(query, &names);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let min_coverage = if terms.len() >= 2 { 2 } else { 1 };
    let mut frontier: Vec<State> = seeds
        .iter()
        .map(|id| State {
            entity: *id,
            covered: 0,
            score: 1.0,
            steps: Vec::new(),
        })
        .collect();
    frontier.sort_by_key(|state| state.entity);
    frontier.dedup_by_key(|state| state.entity);

    let mut cache: HashMap<i64, Vec<Edge>> = HashMap::new();
    let mut hits: Vec<PathHit> = Vec::new();
    for _ in 0..MAX_DEPTH {
        let mut next: Vec<State> = Vec::new();
        for state in &frontier {
            for edge in incident(db, state.entity, vis, &terms, &mut cache)? {
                let covered = state.covered | edge.mask;
                if covered == state.covered {
                    // Every hop must earn its place with a new question term.
                    continue;
                }
                let mut steps = state.steps.clone();
                steps.push(Step {
                    relation_id: edge.relation_id,
                    src: edge.src,
                    dst: edge.dst,
                    predicate: edge.predicate.clone(),
                    fact: edge.fact.clone(),
                    reverse: edge.reverse,
                    matched: edge.mask.count_ones() as usize,
                });
                next.push(State {
                    entity: edge.other,
                    covered,
                    score: state.score * edge.quality,
                    steps,
                });
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_by(|a, b| {
            a.entity
                .cmp(&b.entity)
                .then(a.covered.cmp(&b.covered))
                .then(b.score.total_cmp(&a.score))
        });
        next.dedup_by(|a, b| a.entity == b.entity && a.covered == b.covered);
        for state in &next {
            let coverage = state.covered.count_ones() as usize;
            if coverage >= min_coverage {
                hits.push(PathHit {
                    steps: state.steps.clone(),
                    coverage,
                    score: state.score,
                });
            }
        }
        next.sort_by(|a, b| {
            b.covered
                .count_ones()
                .cmp(&a.covered.count_ones())
                .then(b.score.total_cmp(&a.score))
                .then(a.steps.len().cmp(&b.steps.len()))
        });
        next.truncate(BEAM);
        frontier = next;
    }

    hits.sort_by(|a, b| {
        b.coverage
            .cmp(&a.coverage)
            .then(b.score.total_cmp(&a.score))
            .then(a.steps.len().cmp(&b.steps.len()))
    });
    let mut seen: HashSet<(i64, usize)> = HashSet::new();
    hits.retain(|hit| {
        let end = hit
            .steps
            .last()
            .map(|step| if step.reverse { step.src } else { step.dst })
            .unwrap_or(0);
        seen.insert((end, hit.coverage))
    });
    hits.truncate(PATH_HITS);
    Ok(hits)
}

/// Evidence memories for ranked paths: the destination first, then the links.
/// Deeper coverage wins: a three-hop chain that explains the whole question
/// outranks a two-hop fragment with a slightly better edge score.
pub(crate) fn evidence(db: &Db, hits: &[PathHit]) -> Result<Vec<i64>> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    let mut st = db
        .sql()
        .prepare_cached("SELECT memory_id FROM relation_evidence WHERE relation_id=?1")?;
    for hit in hits {
        let last = hit.steps.len().saturating_sub(1);
        let base = hit.score * (1.0 + hit.coverage as f64).powi(2);
        for (position, step) in hit.steps.iter().enumerate() {
            let weight = base * if position == last { 3.0 } else { 1.0 };
            for memory_id in st.query_map([step.relation_id], |r| r.get::<_, i64>(0))? {
                let memory_id = memory_id?;
                scores
                    .entry(memory_id)
                    .and_modify(|current| *current = current.max(weight))
                    .or_insert(weight);
            }
        }
    }
    let mut ranked: Vec<(i64, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked.into_iter().map(|(id, _)| id).collect())
}

/// The relations along the best paths, destination hop first, ready to render
/// next to the retrieved memories.
pub(crate) fn facts(db: &Db, hits: &[PathHit], cutoff: i64, limit: usize) -> Result<Vec<FactHit>> {
    let mut st = db.sql().prepare_cached(
        "SELECT p.canonical,r.fact,r.valid_from,r.valid_to,r.expired_at,s.name,d.name
         FROM relations r
         JOIN predicates p ON p.id=r.predicate_id
         JOIN entities s ON s.id=r.src
         JOIN entities d ON d.id=r.dst
         WHERE r.id=?1",
    )?;
    let mut seen: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for hit in hits {
        for step in hit.steps.iter().rev() {
            if !seen.insert(step.relation_id) {
                continue;
            }
            let Ok(mut rows) = st.query(params![step.relation_id]) else {
                continue;
            };
            let Some(row) = rows.next()? else { continue };
            let (predicate, fact, valid_from, valid_to, expired, source, target) = (
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            );
            if valid_from > cutoff {
                continue;
            }
            out.push(FactHit {
                id: step.relation_id,
                source,
                predicate,
                target,
                fact,
                valid_from,
                valid_to,
                ended: expired.is_some() || valid_to.is_some_and(|v| v <= cutoff),
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------- tools

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FollowInput {
    /// Entity to start from (name, alias or e<id>).
    pub from: String,
    /// Stop when this entity is reached; reports `reached`.
    #[serde(default)]
    pub to: Option<String>,
    /// Only follow relations whose predicate or fact matches these words.
    #[serde(default)]
    pub predicate: Option<String>,
    /// Hops to walk (1..=3, default 2).
    #[serde(default)]
    pub depth: Option<usize>,
    /// Max facts to return (default 20).
    #[serde(default)]
    pub k: Option<usize>,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub include_history: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FollowFact {
    pub relation: String,
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub fact: String,
    pub valid_from: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    pub ended: bool,
    /// "out" when the relation points away from the reached entity.
    pub direction: String,
    /// Hops from `from` (1 = direct).
    pub hops: usize,
    pub memories: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FollowOutput {
    pub from: String,
    pub to: Option<String>,
    /// The target was reached within `depth` hops.
    pub reached: bool,
    pub facts: Vec<FollowFact>,
    /// Facts reachable within depth before `k` was applied.
    pub total: usize,
    pub truncated: bool,
}

fn visibility(engine: &Engine, as_of: Option<&str>, history: bool) -> Result<Visibility> {
    let clock = engine.clock();
    let as_of = as_of
        .map(|value| {
            crate::temporal::parse_instant(value, &clock)
                .ok_or_else(|| crate::err!("invalid time '{value}'"))
        })
        .transpose()?;
    Ok(Visibility {
        now: clock.now,
        as_of,
        history: history || as_of.is_some(),
    })
}

/// Walk live relations out of one entity, hop by hop. This is the tool an agent
/// uses to finish a chain itself when `recall` returns the neighbourhood but
/// not the answer.
pub fn follow(engine: &Engine, follow: &FollowInput) -> Result<FollowOutput> {
    let db = &engine.db;
    let depth = follow.depth.unwrap_or(2).clamp(1, 3);
    let k = follow.k.unwrap_or(20).clamp(1, 100);
    let vis = visibility(engine, follow.as_of.as_deref(), follow.include_history)?;
    let start = crate::recall::resolve_entity(engine, &follow.from)?;
    let target = follow
        .to
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(|name| crate::recall::resolve_entity(engine, name))
        .transpose()?;
    let want: Vec<HashSet<String>> = follow
        .predicate
        .as_deref()
        .map(|value| query_terms(value, &[]))
        .unwrap_or_default();
    let start_name = pick_name(db, start)?;
    let mut frontier: Vec<i64> = vec![start];
    let mut seen_entities: HashSet<i64> = frontier.iter().copied().collect();
    let mut seen_relations: HashSet<i64> = HashSet::new();
    let mut out: Vec<FollowFact> = Vec::new();
    let mut reached = Some(start) == target;
    for hop in 1..=depth {
        let mut next = Vec::new();
        for entity in &frontier {
            let mut st = db.sql().prepare_cached(
                "SELECT r.id,r.src,r.dst,p.canonical,r.fact,r.valid_from,r.valid_to,r.expired_at,
                        s.name,d.name
                 FROM relations r
                 JOIN predicates p ON p.id=r.predicate_id
                 JOIN entities s ON s.id=r.src
                 JOIN entities d ON d.id=r.dst
                 WHERE r.src=?1 OR r.dst=?2",
            )?;
            let rows = st.query_map(params![entity, entity], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                ))
            })?;
            for row in rows {
                let (id, src, dst, predicate, fact, valid_from, valid_to, expired, sname, dname) =
                    row?;
                if !vis.shows(valid_from, valid_to, expired) || !seen_relations.insert(id) {
                    continue;
                }
                if !want.is_empty() {
                    let forms = text_forms(&format!("{predicate} {fact}"));
                    if !want.iter().any(|term| !term.is_disjoint(&forms)) {
                        continue;
                    }
                }
                let (other, direction) = if *entity == src {
                    (dst, "out")
                } else {
                    (src, "in")
                };
                let memories: Vec<i64> = db
                    .sql()
                    .prepare_cached("SELECT memory_id FROM relation_evidence WHERE relation_id=?1")?
                    .query_map([id], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<_>>()?;
                out.push(FollowFact {
                    relation: format!("r{id}"),
                    source: sname,
                    predicate,
                    target: dname,
                    fact,
                    valid_from,
                    valid_to,
                    ended: expired.is_some() || valid_to.is_some_and(|v| v <= vis.now),
                    direction: direction.to_string(),
                    hops: hop,
                    memories,
                });
                if target == Some(other) {
                    reached = true;
                }
                if seen_entities.insert(other) {
                    next.push(other);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() || (target.is_some() && reached) {
            break;
        }
    }
    let total = out.len();
    // Forward edges first at the same hop: a hub entity reached by one hop
    // ("Yash lives in Nairobi") has hundreds of reverse edges ("X lives in
    // Nairobi") that would otherwise fill `k` before the next forward hop.
    out.sort_by(|a, b| {
        a.hops
            .cmp(&b.hops)
            .then((a.direction == "in").cmp(&(b.direction == "in")))
            .then(a.ended.cmp(&b.ended))
    });
    out.truncate(k);
    Ok(FollowOutput {
        from: start_name,
        to: follow.to.clone(),
        reached,
        facts: out,
        total,
        truncated: total > k,
    })
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AggregateInput {
    /// Relation the question is about ("likes", "pets", "works at").
    #[serde(default)]
    pub predicate: Option<String>,
    /// Count targets of this entity.
    #[serde(default)]
    pub source: Option<String>,
    /// Count sources that reach this entity.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub include_history: bool,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AggregateItem {
    pub name: String,
    pub id: i64,
    pub facts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AggregateOutput {
    pub predicate: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
    /// Distinct entities, not facts.
    pub count: usize,
    pub items: Vec<AggregateItem>,
    /// Facts that explicitly state the opposite (text-level cue).
    pub negative: Vec<AggregateItem>,
    pub memories: Vec<i64>,
    pub truncated: bool,
}

fn pick_name(db: &Db, id: i64) -> Result<String> {
    Ok(db
        .sql()
        .query_row("SELECT name FROM entities WHERE id=?1", [id], |r| r.get(0))?)
}

/// Count or list the targets (or sources) of live relations, deduplicated by
/// entity. This answers "how many", "which ones" and "list" questions that a
/// ranked retrieval cannot answer by ranking alone.
pub fn aggregate(engine: &Engine, input: &AggregateInput) -> Result<AggregateOutput> {
    let db = &engine.db;
    let vis = visibility(engine, None, input.include_history)?;
    let source = input
        .source
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| crate::recall::resolve_entity(engine, value))
        .transpose()?;
    let target = input
        .target
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| crate::recall::resolve_entity(engine, value))
        .transpose()?;
    let terms = input
        .predicate
        .as_deref()
        .map(|value| query_terms(value, &[]))
        .unwrap_or_default();
    if source.is_none() && target.is_none() && terms.is_empty() {
        return Err(crate::err!(
            "aggregate needs a predicate, a source or a target"
        ));
    }
    let limit = input.limit.unwrap_or(50).clamp(1, 200);
    let mut st = db.sql().prepare_cached(
        "SELECT r.id,r.src,r.dst,p.canonical,r.fact,r.valid_from,r.valid_to,r.expired_at,
                s.name,d.name
         FROM relations r
         JOIN predicates p ON p.id=r.predicate_id
         JOIN entities s ON s.id=r.src
         JOIN entities d ON d.id=r.dst
         WHERE (?1 IS NULL OR r.src=?1) AND (?2 IS NULL OR r.dst=?2)
         ORDER BY r.id",
    )?;
    let rows = st.query_map(params![source, target], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut items: HashMap<i64, AggregateItem> = HashMap::new();
    let mut negative: HashMap<i64, AggregateItem> = HashMap::new();
    let mut memories: Vec<i64> = Vec::new();
    for row in rows {
        let (id, src, dst, predicate, fact, valid_from, valid_to, expired, sname, dname) = row?;
        if !vis.shows(valid_from, valid_to, expired) {
            continue;
        }
        if !terms.is_empty() {
            let forms = text_forms(&format!("{predicate} {fact}"));
            if !terms.iter().all(|term| !term.is_disjoint(&forms)) {
                continue;
            }
        }
        // Count the end the question is not anchored on.
        let (other, other_name) = if source.is_some() {
            (dst, dname)
        } else {
            (src, sname)
        };
        let bucket = if has_negation_cue(&fact) {
            &mut negative
        } else {
            &mut items
        };
        bucket
            .entry(other)
            .or_insert_with(|| AggregateItem {
                name: other_name,
                id: other,
                facts: Vec::new(),
            })
            .facts
            .push(fact);
        if memories.len() < 20 {
            let evidence: Vec<i64> = db
                .sql()
                .prepare_cached("SELECT memory_id FROM relation_evidence WHERE relation_id=?1")?
                .query_map([id], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<_>>()?;
            memories.extend(evidence);
        }
    }
    memories.sort_unstable();
    memories.dedup();
    let total = items.len();
    let mut items: Vec<AggregateItem> = items.into_values().collect();
    let mut negative: Vec<AggregateItem> = negative.into_values().collect();
    for list in [&mut items, &mut negative] {
        list.sort_by(|a, b| a.name.cmp(&b.name));
    }
    items.truncate(limit);
    negative.truncate(limit);
    Ok(AggregateOutput {
        predicate: input.predicate.clone(),
        source: input
            .source
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string),
        target: input
            .target
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string),
        count: if source.is_some() && target.is_some() {
            usize::from(!items.is_empty())
        } else {
            total
        },
        items,
        negative,
        memories,
        truncated: total > limit,
    })
}
