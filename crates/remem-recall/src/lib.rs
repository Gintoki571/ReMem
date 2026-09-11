//! ReMem v3 recall: weighted RRF fusion over FTS + vector (+ optional graph)
//! lists, a recency band on the fused score, and a Store+Graph facade.

pub mod rank;
pub mod stub;

pub use rank::{
    apply_floor, cosine, final_score, fuse, is_content_free, pack_by_budget, recency_score, rrf,
    tag_boost, tokens, Fused, Ranking, DEFAULT_COSINE_FLOOR, DEFAULT_HALF_LIFE_DAYS, DEFAULT_RRF_K,
};
pub use stub::StubEmbedder;

use anyhow::{anyhow, Context, Result};
use remem_graph::{EdgeProvenance, Graph, MEMORY_LABEL};
use remem_store::{content_hash, Store};
use remem_types::{MemoryItem, RecallHit, RecallQuery};
use std::collections::{HashMap, HashSet};

/// Distinctive objects of a memory (issue #11 corroboration): the tags, plus
/// identifier spans (all-digit tokens, e.g. "8080", "v3"), plus proper nouns
/// (capitalized words that are not the first word of the content).
/// ponytail: capitalization heuristic, not NER — upgrade to a real NER pass if
/// a corpus needs it; tags and identifiers already carry most of the signal.
fn distinctive_objects(item: &MemoryItem) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> = item.tags.iter().cloned().collect();
    for (i, tok) in item
        .content
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .enumerate()
    {
        if tok.chars().all(|c| c.is_ascii_digit())
            || (i > 0 && tok.chars().next().is_some_and(|c| c.is_uppercase()))
        {
            out.insert(tok.to_string());
        }
    }
    out
}

/// Minimal local embedding contract so this crate is not blocked by
/// remem-embed. Vectors must be 768-dim (the store's vec0 width); the real
/// embedder is wired in at integration.
pub trait Embed {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

/// How many top seeds get one graph hop during expansion.
const GRAPH_EXPANSION_SEEDS: usize = 8;

/// Default score floor: 0.0 means off, so recall is unchanged unless a caller
/// opts in (`--min-score`). Re-measured on docs/eval-fixtures.json (40 memories,
/// debug binary, Cpu 768d embedder, fresh db) at the k=30 / fts 1.0 /
/// vector 0.5 weights: the 3 pure-stopword adversarial queries (`expect: ""`)
/// top out at 0.0161-0.0169, and the lowest top-1 over all 37 answerable
/// queries is 0.0438, so the usable band is 0.017..0.043. 0.02 sits inside it
/// and still drops all junk while keeping every answerable top-1; it is no
/// longer the midpoint of the band, but moving it is not warranted by one
/// corpus. It stays off by default because a floor that is wrong for one corpus
/// silently returns [].
pub const DEFAULT_MIN_SCORE: f64 = 0.0;

/// Max L2 distance for a `remember` near-duplicate report. Vectors are
/// L2-normalized, so d = sqrt(2 - 2 cos): d = 0.48 is cos ~ 0.885.
///
/// Calibration (`crates/remem-recall/examples/near-dup-calibrate.rs`, real
/// Cpu 768d embedder over all 40 docs/eval-fixtures.json memories, 780 pairs):
/// the 4 deliberate paraphrase pairs measure 0.4176, 0.4313, 0.4331 and
/// 0.6349; the closest NON-paraphrase pair (two "docs live in the wiki"
/// notes) measures 0.5183. So no single threshold fires on all four
/// paraphrases while staying silent on every distinct pair — the 3-against-1
/// split is what a tight threshold buys. 0.48 sits in the gap: it fires on 3
/// of 4 paraphrases (misses only 22/35, the loosest rewrite) and stays silent
/// on all 776 distinct pairs. Below it the signal degrades to noise; a writer
/// shown near-duplicates on every second save will ignore them. 0.48 is also
/// the round midpoint of the usable 0.44..0.51 band. Re-check on a new corpus
/// before moving it: the gap is fixture-specific, not universal.
pub const SIMILAR_MAX_DISTANCE: f32 = 0.48;

/// How many neighbours the write probe pulls before thresholding. 3 per the
/// tracker; a 4th near-dup would itself be a re-save worth seeing, but the
/// writer only needs enough to recognize the duplicate.
const SIMILAR_PROBE_K: usize = 3;

/// Per-list depth for candidate generation. bm25 and knn both truncate here;
/// `4 * k` leaves room for fusion to disagree. ponytail: constant depth, no
/// adaptive recall until ranking quality is measured on real data.
const LIST_DEPTH_FACTOR: usize = 4;

/// Relative weight of each ranked list in the fusion.
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub fts: f64,
    pub vector: f64,
    pub graph: f64,
}

impl Default for Weights {
    /// Winner of the docs/weight-spike.md grid search (35/37 recall@1 vs 31/37
    /// at 1.0/1.0): the lexical list decides, the embedder gets a half-weight
    /// vote. Graph stays 1.0 — the fixtures carry no memory-to-memory edges, so
    /// the spike learned nothing about it and there is no evidence to move it.
    fn default() -> Self {
        Self {
            fts: 1.0,
            vector: 0.5,
            graph: 1.0,
        }
    }
}

/// Store + Graph + embedder, ranked recall on top.
///
/// Heavy resources live behind one `Arc`, so the scoring knobs
/// ([`RecallEngine::with_half_life_days`], [`RecallEngine::with_min_score`],
/// [`RecallEngine::with_recency_off`]) derive a re-tuned engine from `&self`
/// without re-opening store or graph. The construction-time builders
/// One node of an [`RecallEngine::impact`] walk: a memory reached over one
/// memory-to-memory edge, with the edge and BFS position that found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactNode {
    /// Memory id.
    pub id: String,
    /// Memory kind string (e.g. `fact`), empty when the graph node has no
    /// store row (forgotten memory kept only in the graph).
    pub kind: String,
    /// Relation on the discovering edge (`SUPERSEDES`, `USES`, ...).
    pub rel: String,
    /// Provenance of the discovering edge.
    pub provenance: EdgeProvenance,
    /// BFS depth from the seed (first hop = 1).
    pub depth: usize,
}

/// Result of a directional BFS over memory-to-memory edges from a seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactReport {
    /// The seed memory id.
    pub root: String,
    /// Discovered nodes, breadth-first (depth ascending, discovery order
    /// within a depth). The root itself is not listed.
    pub nodes: Vec<ImpactNode>,
}

pub(crate) const DEFAULT_IMPACT_DEPTH: usize = 3;

/// ([`RecallEngine::with_graph`], [`RecallEngine::with_weights`]) still consume
/// `self`: they shape the engine, not a recall variant of it.
pub struct RecallEngine {
    shared: std::sync::Arc<Shared>,
    weights: Weights,
    half_life_days: f64,
    min_score: f64,
    cosine_floor: f32,
    recency_enabled: bool,
}

struct Shared {
    store: Store,
    graph: Option<Graph>,
    embed: Box<dyn Embed>,
}

impl RecallEngine {
    /// Engine over an open store with no graph projection.
    pub fn new(store: Store, embed: Box<dyn Embed>) -> Self {
        Self {
            shared: std::sync::Arc::new(Shared {
                store,
                graph: None,
                embed,
            }),
            weights: Weights::default(),
            half_life_days: DEFAULT_HALF_LIFE_DAYS,
            min_score: DEFAULT_MIN_SCORE,
            cosine_floor: rank::DEFAULT_COSINE_FLOOR,
            recency_enabled: true,
        }
    }

    /// Enable graph projection and neighbour expansion on the same db file.
    pub fn with_graph(mut self, graph: Graph) -> Self {
        if let Some(s) = std::sync::Arc::get_mut(&mut self.shared) {
            s.graph = Some(graph);
        }
        self
    }

    pub fn with_weights(mut self, weights: Weights) -> Self {
        self.weights = weights;
        self
    }

    /// Config clone with shared resources: the base for every derived knob.
    fn derive(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            weights: self.weights,
            half_life_days: self.half_life_days,
            min_score: self.min_score,
            cosine_floor: self.cosine_floor,
            recency_enabled: self.recency_enabled,
        }
    }

    /// Recency half-life override, derived from `&self` (shared resources).
    pub fn with_half_life_days(&self, days: f64) -> Self {
        let mut e = self.derive();
        e.half_life_days = days;
        e
    }

    /// Drop recall hits scoring below `min_score`. `0.0` disables the floor.
    pub fn with_min_score(&self, min_score: f64) -> Self {
        let mut e = self.derive();
        e.min_score = min_score;
        e
    }

    /// Recency off: skip the recency band entirely (no `recent` reasons, no
    /// recency multiplier). Derived from `&self` so it composes with the other
    /// knobs (docs/temporal-eval.md: the fixtures are timeless; callers that
    /// know their corpus has no time signal can turn the band off).
    pub fn with_recency_off(&self) -> Self {
        let mut e = self.derive();
        e.recency_enabled = false;
        e
    }

    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn graph(&self) -> Option<&Graph> {
        self.shared.graph.as_ref()
    }

    pub fn weights(&self) -> &Weights {
        &self.weights
    }

    /// Embed `texts` in one batch, checking the embedder kept its contract.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let out = self.shared.embed.embed(texts)?;
        if out.len() != texts.len() {
            return Err(anyhow!(
                "embedder returned {} vectors for {} texts",
                out.len(),
                texts.len()
            ));
        }
        Ok(out)
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed_batch(&[text])?;
        Ok(v.pop().expect("length checked above"))
    }

    /// Insert, project onto the graph, and index the embedding. Returns the
    /// id plus near-duplicate neighbours probed (before insert) against the
    /// already-stored embeddings: up to `SIMILAR_PROBE_K` memories closer than
    /// [`SIMILAR_MAX_DISTANCE`], as (id, L2 distance). Exact duplicates
    /// dedup on content_hash and report nothing (the store returns their id).
    pub fn remember(&self, item: &MemoryItem) -> Result<(String, Vec<(String, f32)>)> {
        let vec = self.embed_one(&item.content)?;
        let is_dup = self
            .shared
            .store
            .find_by_hash(&content_hash(&item.kind, &item.content))
            .is_some();
        let similar: Vec<(String, f32)> = if is_dup {
            Vec::new()
        } else {
            self.shared
                .store
                .knn(&vec, SIMILAR_PROBE_K)?
                .into_iter()
                .filter(|(_id, dist)| *dist <= SIMILAR_MAX_DISTANCE)
                .collect()
        };
        // Corroboration gate (issue #11): a near neighbour only absorbs this
        // write when it is the UNIQUE candidate whose distinctive objects
        // (tags + identifier spans + proper nouns) all reappear in the new
        // item. Zero corroborators (new fact) or two-plus (ambiguous) split
        // into a new row; a value correction ("8080" -> "9090") never has its
        // old objects reproduced, so it always splits. Live gate, no
        // kill-switch.
        let objs = distinctive_objects(item);
        let corroborating: Vec<String> = similar
            .iter()
            .filter(|(id, _)| {
                self.shared
                    .store
                    .get(id)
                    .ok()
                    .flatten()
                    .map(|cand| {
                        let co = distinctive_objects(&cand);
                        !co.is_empty() && co.is_subset(&objs)
                    })
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        if corroborating.len() == 1 {
            // Merge: the existing row already says this; no insert, no graph
            // node, keep the old (original) id.
            let survivor = corroborating.into_iter().next().expect("len == 1");
            // Traceable merges (issue #14): keep the absorbed phrasing so
            // recall and `remem trace` can still surface it.
            self.shared
                .store
                .insert_merge(&survivor, &item.content, MemoryItem::now())
                .context("record merge")?;
            return Ok((survivor, similar));
        }
        let id = self.shared.store.insert(item).context("store insert")?;
        self.shared
            .store
            .set_embedding(&id, &vec)
            .context("set_embedding")?;
        if let Some(g) = &self.shared.graph {
            g.attach(item).map_err(|e| anyhow!("graph attach: {e}"))?;
        }
        Ok((id, similar))
    }

    /// Remove a memory from store and graph.
    pub fn forget(&self, id: &str) -> Result<()> {
        self.shared.store.delete(id).context("store delete")?;
        if let Some(g) = &self.shared.graph {
            g.forget(id).map_err(|e| anyhow!("graph forget: {e}"))?;
        }
        Ok(())
    }

    /// Directed edge between two memories. Nodes must already be attached.
    /// Records `manual` provenance.
    pub fn link(&self, from: &str, to: &str, rel: Option<&str>) -> Result<()> {
        self.link_with_provenance(from, to, rel, EdgeProvenance::Manual.as_str())
    }

    /// Directed edge with an explicit provenance
    /// (`manual | recall-suggested | correction-chain`); anything else errors.
    pub fn link_with_provenance(
        &self,
        from: &str,
        to: &str,
        rel: Option<&str>,
        provenance: &str,
    ) -> Result<()> {
        let g = self
            .shared
            .graph
            .as_ref()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let rel = rel.unwrap_or(remem_graph::DEFAULT_REL);
        let prov = EdgeProvenance::parse(provenance).ok_or_else(|| {
            anyhow!("unknown provenance: {provenance} (manual|recall-suggested|correction-chain)")
        })?;
        g.link_with_provenance(from, to, rel, prov)
            .map_err(|e| anyhow!("graph link: {e}"))
    }

    /// Correction write (issue #15): supersede the old row in the store, then
    /// project the replacement onto the graph with a `correction-chain`
    /// `SUPERSEDES` edge from the new version to the old one. The old graph
    /// node stays (trace/audit), like its store row.
    pub fn supersede(&self, old_id: &str, replacement: &MemoryItem) -> Result<String> {
        let id = self
            .shared
            .store
            .supersede(old_id, replacement)
            .context("store supersede")?;
        if let Some(g) = &self.shared.graph {
            g.attach(replacement)
                .map_err(|e| anyhow!("graph attach: {e}"))?;
            g.link_with_provenance(
                &replacement.id,
                old_id,
                "SUPERSEDES",
                EdgeProvenance::CorrectionChain,
            )
            .map_err(|e| anyhow!("graph link: {e}"))?;
        }
        Ok(id)
    }

    /// `SUPERSEDES` edges whose live endpoints share no tags, e.g.
    /// `suspect SUPERSEDES: m1 -> m2 (no shared tags)`. A correction should
    /// share its topic markers; a cross-topic link is the report's mis-link
    /// probe and is surfaced here instead of silently fusing. Genuine
    /// corrections never appear: their old row is superseded (hidden from
    /// `get`), so only live-live edges are judged. Both-untagged pairs carry
    /// no signal and stay silent.
    pub fn suspect_supersedes(&self) -> Result<Vec<String>> {
        let g = self
            .shared
            .graph
            .as_ref()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let edges = g.memory_edges().map_err(|e| anyhow!("graph edges: {e}"))?;
        let mut out = Vec::new();
        for e in edges {
            if e.rel != "SUPERSEDES" {
                continue;
            }
            let (Some(a), Some(b)) = (
                self.shared.store.get(&e.from)?,
                self.shared.store.get(&e.to)?,
            ) else {
                continue;
            };
            let (ta, tb): (HashSet<&str>, HashSet<&str>) = (
                a.tags.iter().map(String::as_str).collect(),
                b.tags.iter().map(String::as_str).collect(),
            );
            if !ta.is_disjoint(&tb) || (ta.is_empty() && tb.is_empty()) {
                continue;
            }
            out.push(format!(
                "suspect SUPERSEDES: {} -> {} (no shared tags)",
                e.from, e.to
            ));
        }
        Ok(out)
    }

    pub fn list(&self) -> Result<Vec<MemoryItem>> {
        Ok(self.shared.store.list(false)?)
    }

    /// Impact of changing `id`: BFS over memory edges to depth 3, all
    /// relations. See [`Self::impact_filtered`].
    pub fn impact(&self, id: &str) -> Result<ImpactReport> {
        self.impact_filtered(id, DEFAULT_IMPACT_DEPTH, None)
    }

    /// Impact with an explicit depth limit.
    pub fn impact_depth(&self, id: &str, depth: usize) -> Result<ImpactReport> {
        self.impact_filtered(id, depth, None)
    }

    /// Directional BFS over memory-to-memory edges (donor: graphify
    /// `affected.py::affected_nodes`): visit both edge directions, filter by
    /// relation when `rel` is set, stop at `depth` hops. Incoming edges are
    /// dependents (what breaks if this memory changes); outgoing edges are
    /// its own dependencies. SUPERSEDES edges surface superseded rows next
    /// to their live successors in either direction. Unknown id -> empty
    /// report (mirrors related/path emptiness).
    pub fn impact_filtered(
        &self,
        id: &str,
        depth: usize,
        rel: Option<&str>,
    ) -> Result<ImpactReport> {
        let g = self
            .shared
            .graph
            .as_ref()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let hits = g
            .impact(id, depth, rel)
            .map_err(|e| anyhow!("graph impact: {e}"))?;
        // Kind comes from the store; superseded rows are hidden from list()
        // but their graph nodes remain, so per-id get is required. A graph
        // node with no row (forgotten memory) degrades to an empty kind.
        let mut nodes = Vec::with_capacity(hits.len());
        for n in hits {
            let kind = match self.shared.store.get_any(&n.id)? {
                Some(item) => item.kind.as_str().to_string(),
                None => String::new(),
            };
            nodes.push(ImpactNode {
                id: n.id,
                kind,
                rel: n.rel,
                provenance: n.provenance,
                depth: n.depth,
            });
        }
        Ok(ImpactReport {
            root: id.to_string(),
            nodes,
        })
    }

    /// Combined counts for `remem stats`.
    pub fn stats(&self) -> Result<serde_json::Value> {
        let memories = self.shared.store.list(false)?.len();
        let mut out = serde_json::json!({ "memories": memories });
        if let Some(g) = &self.shared.graph {
            let s = g.stats().map_err(|e| anyhow!("graph stats: {e}"))?;
            out["graph"] = s;
        }
        Ok(out)
    }

    /// Ranked recall over FTS, vector and (optionally) graph expansion.
    /// `k == 0` means "no results": returns [] before any FTS/vector work.
    /// (list/central/path limits are separate paths and keep their defaults.)
    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<RecallHit>> {
        if query.k == 0 {
            return Ok(Vec::new());
        }
        let text = query.text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        // reason: query-empty. No FTS/vector calls: content-free queries have
        // no lexical anchor, so any hit would be arbitrary (adversarial-battery #4).
        if is_content_free(text) {
            return Ok(Vec::new());
        }
        let k = query.k;
        let depth = LIST_DEPTH_FACTOR * k;
        let now = MemoryItem::now();

        // Filtered id set; an unfiltered list would leak soft-scoped hits.
        let allowed: HashSet<String> = self
            .shared
            .store
            .list(false)?
            .into_iter()
            .filter(|m| matches(m, query))
            .map(|m| m.id)
            .collect();

        let fts: Vec<String> = self
            .shared
            .store
            .fts_search(text, depth)?
            .into_iter()
            .map(|(m, _bm25)| m.id)
            .filter(|id| allowed.contains(id))
            .collect();

        // Traceable merges (issue #14): absorbed phrasings are searchable
        // too. Survivors whose merged-in content matched enter the lexical
        // list AFTER the direct fts hits, so direct matches keep their rank
        // and the eval ordering cannot regress from a merge hit alone.
        let mut fts = fts;
        for (sid, _rank) in self.shared.store.fts_search_merges(text, depth)? {
            if allowed.contains(&sid) && !fts.contains(&sid) {
                fts.push(sid);
            }
        }

        let qvec = self.embed_one(text)?;

        // Cosine floor (issue #9): two guards on the query, judged against
        // vector geometry, not fused ranks.
        // R1 - abstention: no lexical anchor at all, and even the closest
        // stored vector sits below the floor -> the query is junk relative to
        // this corpus; every hit downstream would be arbitrary. Returns [].
        // R2 - list pruning: a STRONG lexical anchor (fts#1 content at/above
        // the floor) means the query is real, so vector-only candidates below
        // the floor are noise riding the knn tail and are dropped from the
        // vector list. A weak lexical anchor prunes nothing: the embedder may
        // simply not carry the signal (tag-only or graph-only recall must
        // survive), and the fts list already vouches for the query.
        // knn returns L2 distance on L2-normalized vectors, so
        // cosine = 1 - d^2/2 (d=0 -> 1.0, orthogonal sqrt(2) -> 0.0).
        let knn: Vec<(String, f32)> = self
            .shared
            .store
            .knn(&qvec, depth)?
            .into_iter()
            .filter(|(id, _)| allowed.contains(id))
            .collect();
        let knn_cosine = |d: f32| 1.0 - d * d / 2.0;
        if fts.is_empty() && self.cosine_floor > 0.0 {
            let below = knn
                .first()
                .map_or(true, |(_, d)| knn_cosine(*d) < self.cosine_floor);
            if below {
                return Ok(Vec::new());
            }
        }
        let vector: Vec<String> = if !fts.is_empty() && self.cosine_floor > 0.0 {
            let anchored = match fts.first() {
                Some(id) => match self.shared.store.get(id)? {
                    Some(item) => {
                        let av = self.embed_one(&item.content)?;
                        rank::cosine(&qvec, &av) >= self.cosine_floor
                    }
                    None => false,
                },
                None => false,
            };
            if anchored {
                knn.into_iter()
                    .filter(|(_, d)| knn_cosine(*d) >= self.cosine_floor)
                    .map(|(id, _)| id)
                    .collect()
            } else {
                knn.into_iter().map(|(id, _)| id).collect()
            }
        } else {
            knn.into_iter().map(|(id, _)| id).collect()
        };

        // Graph expansion: neighbours of the top fts+vector seeds, fused as a
        // third list so linked-but-not-matched memories can surface.
        let mut edge_prov: HashMap<String, EdgeProvenance> = HashMap::new();
        // recall-suggested candidates (issue #15 follow-up): seed that pulled
        // each graph-fused id in (first seed wins).
        let mut edge_from: HashMap<String, String> = HashMap::new();
        let graph_ids: Vec<String> = if let Some(g) = &self.shared.graph {
            let seeds = fuse(
                &[
                    Ranking {
                        name: "fts",
                        ids: &fts,
                        weight: self.weights.fts,
                    },
                    Ranking {
                        name: "vector",
                        ids: &vector,
                        weight: self.weights.vector,
                    },
                ],
                DEFAULT_RRF_K,
            );
            let mut seen = HashSet::new();
            let mut nbrs = Vec::new();
            for seed in seeds.iter().take(k.min(GRAPH_EXPANSION_SEEDS)) {
                for n in g
                    .neighbors_detail(&seed.id)
                    .map_err(|e| anyhow!("graph neighbors: {e}"))?
                {
                    if !n.labels.iter().any(|l| l == MEMORY_LABEL) {
                        continue;
                    }
                    if allowed.contains(&n.id) && seen.insert(n.id.clone()) {
                        edge_prov.insert(n.id.clone(), n.provenance);
                        edge_from.insert(n.id.clone(), seed.id.clone());
                        nbrs.push(n.id);
                    }
                }
            }
            nbrs
        } else {
            Vec::new()
        };

        let mut lists = vec![
            Ranking {
                name: "fts",
                ids: &fts,
                weight: self.weights.fts,
            },
            Ranking {
                name: "vector",
                ids: &vector,
                weight: self.weights.vector,
            },
        ];
        if !graph_ids.is_empty() {
            lists.push(Ranking {
                name: "graph",
                ids: &graph_ids,
                weight: self.weights.graph,
            });
        }

        let fused = fuse(&lists, DEFAULT_RRF_K);
        let mut hits: Vec<RecallHit> = Vec::new();
        for entry in fused {
            // Strict decode: skip a row that vanished or fails to decode
            // instead of failing the whole recall.
            let Ok(Some(item)) = self.shared.store.get(&entry.id) else {
                continue;
            };
            // Two clocks: rank by when it happened, not when it was typed.
            // with_recency_off pins the band at 1.0: no multiplier, no `recent`
            // reasons (docs/temporal-eval.md fixtures carry no time signal).
            let recency = if self.recency_enabled {
                recency_score(item.event_time(), self.half_life_days, now)
            } else {
                1.0
            };
            let mut reasons = entry.reasons;
            // Issue #15: a graph-fused hit names the provenance of the edge
            // that pulled it in (`prov:manual`, ...). Always present on graph#
            // hits so callers can distrust non-manual evidence at a glance.
            if reasons.iter().any(|r| r.starts_with("graph#")) {
                if let Some(p) = edge_prov.get(&entry.id) {
                    reasons.push(format!("prov:{}", p.as_str()));
                }
            }
            if self.recency_enabled && recency > 0.9 {
                reasons.push("recent".to_string());
            }
            if item.importance >= 0.8 {
                reasons.push("important".to_string());
            }
            let score = final_score(entry.score, recency);
            hits.push(RecallHit {
                item,
                score,
                reasons,
            });
        }
        // Floor first, boost second. The tag boost is a 2.0x multiplier, so a
        // hit that is junk on its own merits can be multiplied past the floor
        // (Q39: junk at 0.0161/0.0122 pre-boost passing a 0.017 floor at
        // 0.0323/0.0244). Flooring the unboosted score means the floor judges
        // the fusion+recency evidence and the boost can only lift something
        // already above it. Boosting first would make the floor a statement
        // about tags, not about relevance. Order: rank -> truncate -> floor ->
        // boost -> re-sort -> budget pack. Pack stays last because the boost
        // can change which hit is the top hit it must always keep.
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(k);
        let mut hits = apply_floor(hits, self.min_score);
        // Tag channel: FTS and the embedder both see content only, so when a
        // query's lexical anchor lives in tags ("decide" vs `decision`) nothing
        // else can rank it. See rank::tag_boost for the matching rules.
        {
            let tagged: Vec<(&str, &[String], bool)> = hits
                .iter()
                .map(|h| {
                    let has_fts = h.reasons.iter().any(|r| r.starts_with("fts#"));
                    (h.item.id.as_str(), &h.item.tags[..], has_fts)
                })
                .collect();
            let factors: Vec<f64> = tag_boost(&tagged, &tokens(text))
                .into_iter()
                .map(|(_, f)| f)
                .collect();
            for (hit, factor) in hits.iter_mut().zip(factors) {
                if factor > 1.0 {
                    hit.score *= factor;
                    hit.reasons.push("tag".to_string());
                }
            }
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        let hits = match query.max_chars {
            Some(max) => pack_by_budget(hits, max),
            None => hits,
        };
        // recall-suggested edge candidates (issue #15 follow-up): when a graph
        // edge fused a hit in, record the (seed -> hit) pair + query + rank as
        // a candidate for the agent to accept with `link --rel`. SUGGESTION
        // ONLY: never auto-links (that regressed once). Failures are non-fatal.
        if !edge_from.is_empty() {
            for (pos, h) in hits.iter().enumerate() {
                if h.reasons.iter().any(|r| r.starts_with("graph#")) {
                    if let Some(from) = edge_from.get(&h.item.id) {
                        let _ = self.shared.store.record_suggestion(
                            from,
                            &h.item.id,
                            text,
                            (pos + 1) as i64,
                        );
                    }
                }
            }
            let _ = self.shared.store.prune_suggestions();
        }
        Ok(hits)
    }

    /// Cross-check live store rows against graph Memory nodes. Reports store
    /// rows with no graph node (`missing graph node: {id}`) and graph nodes
    /// with no live row (`ghost graph node: {id}` — purged without graph
    /// forget). Soft-deleted rows are not ghosts: their nodes are dropped by
    /// design when the memory is forgotten.
    pub fn graph_gaps(&self) -> Result<Vec<String>> {
        let g = self
            .graph()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let live: HashSet<String> = self
            .store()
            .list(false)?
            .into_iter()
            .map(|m| m.id)
            .collect();
        let rows = g.cypher(
            "MATCH (n:Memory) RETURN n.mid AS mid",
            &serde_json::Value::Null,
        )?;
        let mut graph_ids: HashSet<String> = HashSet::new();
        if let Some(list) = rows.as_array() {
            for row in list {
                if let Some(mid) = row.get("mid").and_then(|v| v.as_str()) {
                    graph_ids.insert(mid.to_string());
                }
            }
        }
        let mut gaps = Vec::new();
        for id in &live {
            if !graph_ids.contains(id) {
                gaps.push(format!("missing graph node: {id}"));
            }
        }
        for id in &graph_ids {
            if !live.contains(id) {
                gaps.push(format!("ghost graph node: {id}"));
            }
        }
        gaps.sort();
        Ok(gaps)
    }

    /// Delete ghost graph nodes (nodes with no live store row). Returns the
    /// number removed; store-live nodes are never touched.
    pub fn gc_ghost_nodes(&self) -> Result<usize> {
        let g = self
            .graph()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let rows = g.cypher(
            "MATCH (n:Memory) RETURN n.mid AS mid",
            &serde_json::Value::Null,
        )?;
        let graph_ids: Vec<String> = rows
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|row| row.get("mid").and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let live: HashSet<String> = self
            .store()
            .list(false)?
            .into_iter()
            .map(|m| m.id)
            .collect();
        let mut removed = 0;
        for id in graph_ids {
            if !live.contains(&id) {
                g.forget(&id)
                    .map_err(|e| anyhow!("graph forget {id}: {e}"))?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Reconcile FTS and vec index row counts against live (deleted=0)
    /// memories. FTS is external-content (rowid = memories.rowid), vec is
    /// vec0 keyed by rowid; a live memory with no joinable index row is drift.
    /// Returns issue lines, empty when healthy.
    pub fn index_audit(&self) -> Result<Vec<String>> {
        let conn = self.store().connection();
        let memories: i64 =
            conn.query_row("SELECT COUNT(*) FROM memories WHERE deleted = 0", [], |r| {
                r.get(0)
            })?;
        // FTS external-content probe: one quoted-token MATCH per live memory
        // (first alphanumeric token, rowid-filtered). Token-less content is
        // skipped by design.
        let rows: Vec<(i64, String)> = {
            let mut stmt = conn.prepare("SELECT rowid, content FROM memories WHERE deleted = 0")?;
            rows_of(&mut stmt)?
        };
        let mut fts: i64 = 0;
        for (rowid, content) in &rows {
            let token = content
                .split(|c: char| !c.is_alphanumeric())
                .find(|t| !t.is_empty());
            let Some(token) = token else { continue };
            let found: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH ?1 AND rowid = ?2",
                rusqlite::params![format!("\"{}\"", token.replace('"', "\"\"\"")), rowid],
                |r| r.get(0),
            )?;
            fts += found.min(1);
        }
        let vec: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories m WHERE m.deleted = 0 \
             AND EXISTS (SELECT 1 FROM mem_vec v WHERE v.rowid = m.rowid)",
            [],
            |r| r.get(0),
        )?;
        let mut issues = Vec::new();
        if fts < memories || vec < memories {
            issues.push(format!("memories: {memories}  fts: {fts}  vec: {vec}"));
            if fts < memories {
                issues.push(format!("fts missing: {}", memories - fts));
            }
            if vec < memories {
                issues.push(format!("vec missing: {}", memories - vec));
            }
        }
        Ok(issues)
    }
}

fn matches(item: &MemoryItem, query: &RecallQuery) -> bool {
    if let Some(kinds) = &query.kinds {
        if !kinds.contains(&item.kind) {
            return false;
        }
    }
    if let Some(tags) = &query.tags {
        if !tags.iter().any(|t| item.tags.contains(t)) {
            return false;
        }
    }
    if let Some(a) = &query.agent_id {
        if item.agent_id != *a {
            return false;
        }
    }
    if let Some(sid) = &query.session_id {
        if item.session_id != *sid {
            return false;
        }
    }
    // Future occurred_at rows never belong to a historical window: the clamp
    // pins event_time at "now" for scoring, so window filters must see the
    // same clamped value or a --since/--until pair would exclude-and-include
    // the same row inconsistently.
    let t = item.event_time().min(MemoryItem::now());
    if let Some(since) = &query.since {
        if t < *since {
            return false;
        }
    }
    if let Some(until) = &query.until {
        if t > *until {
            return false;
        }
    }
    true
}

/// Collect a two-column (i64, String) statement result. Local helper so the
/// engine facade needs no rusqlite trait imports for the audit escape hatch.
fn rows_of(stmt: &mut rusqlite::Statement<'_>) -> rusqlite::Result<Vec<(i64, String)>> {
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

#[cfg(test)]
mod impact_tests {
    use super::*;
    use crate::stub::StubEmbedder;
    use remem_graph::Graph;
    use remem_store::Store;
    use remem_types::{MemoryItem, MemoryKind};

    fn test_engine(path: &std::path::Path) -> RecallEngine {
        let store = Store::open_path(path).unwrap();
        let graph = Graph::open(path).unwrap();
        RecallEngine::new(store, Box::new(StubEmbedder)).with_graph(graph)
    }

    fn tmp_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "remem-impact-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("remem.db")
    }

    fn item(kind: MemoryKind, content: &str, tags: &[&str]) -> MemoryItem {
        let mut m = MemoryItem::new(kind, content.to_string());
        m.tags = tags.iter().map(|s| s.to_string()).collect();
        m
    }

    /// Chain fixture: m0 <- m1 (SUPERSEDES, correction-chain) plus an
    /// unrelated live memory m_unlinked.
    fn chain_fixture(path: &std::path::Path) -> (RecallEngine, Vec<String>) {
        let eng = test_engine(path);
        let mut ids = Vec::new();
        let mut prev: Option<MemoryItem> = None;
        for content in [
            "deploy runs on port 8080",
            "deploy runs on port 9090",
            "deploy runs on port 9443",
        ] {
            let mut m = item(MemoryKind::Fact, content, &["deploy"]);
            if let Some(old) = &prev {
                let new_id = eng.supersede(&old.id, &m).expect("supersede succeeds");
                ids.push(new_id.clone());
                m.id = new_id;
            } else {
                eng.remember(&m).expect("remember succeeds");
                ids.push(m.id.clone());
            }
            prev = Some(m);
        }
        let unlinked = item(MemoryKind::Note, "unrelated grocery list", &["groceries"]);
        eng.remember(&unlinked).expect("remember succeeds");
        ids.push(unlinked.id);
        (eng, ids)
    }

    #[test]
    fn correction_chain_root_reports_every_successor() {
        let db = tmp_db("chain-root");
        let (eng, ids) = chain_fixture(&db);
        let report = eng.impact(&ids[0]).expect("impact succeeds");
        // Root is line 0; both successors appear (depth walk is unbounded on
        // SUPERSEDES), the newest-last.
        assert_eq!(report.root, ids[0]);
        let walked: Vec<&str> = report.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            walked.contains(&ids[1].as_str()) && walked.contains(&ids[2].as_str()),
            "both successors must appear: {walked:?}"
        );
        assert!(
            report
                .nodes
                .iter()
                .filter(|n| n.id == ids[2])
                .all(|n| n.depth == 2),
            "newest successor sits two hops out: {:?}",
            report.nodes
        );
        assert!(
            !walked.contains(&ids[3].as_str()),
            "unrelated memory stays out"
        );
        let sup = report
            .nodes
            .iter()
            .find(|n| n.id == ids[1])
            .expect("m1 present");
        assert_eq!(sup.rel, "SUPERSEDES");
        assert_eq!(sup.provenance, remem_graph::EdgeProvenance::CorrectionChain);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn newest_version_walks_back_to_superseded_originals() {
        // Task spec: superseded rows point to live successors — show both.
        // impact on the newest id must surface the older, hidden rows.
        let db = tmp_db("chain-newest");
        let (eng, ids) = chain_fixture(&db);
        let report = eng.impact(&ids[2]).expect("impact succeeds");
        let walked: Vec<&str> = report.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            walked.contains(&ids[0].as_str()) && walked.contains(&ids[1].as_str()),
            "superseded originals must appear from the live tip: {walked:?}"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn fan_out_links_report_each_dependent_breadth_first() {
        let db = tmp_db("fanout");
        let eng = test_engine(&db);
        let base = item(
            MemoryKind::Decision,
            "api base url is https://api.example.com",
            &["api"],
        );
        eng.remember(&base).expect("remember succeeds");
        let mut dependents = Vec::new();
        for content in [
            "client a calls the payments endpoint",
            "client b calls the shipments endpoint",
            "client c calls the audit endpoint",
        ] {
            let m = item(MemoryKind::Fact, content, &["api"]);
            eng.remember(&m).expect("remember succeeds");
            eng.link(&m.id, &base.id, Some("USES"))
                .expect("link succeeds");
            dependents.push(m.id);
        }
        let deep = item(
            MemoryKind::Fact,
            "retry wrapper wraps the payments caller",
            &["api"],
        );
        eng.remember(&deep).expect("remember succeeds");
        eng.link(&deep.id, &dependents[0], Some("USES"))
            .expect("link succeeds");

        let report = eng.impact(&base.id).expect("impact succeeds");
        let get = |id: &str| {
            report
                .nodes
                .iter()
                .find(|n| n.id == id)
                .unwrap_or_else(|| panic!("{id} missing: {:?}", report.nodes))
        };
        for d in &dependents {
            assert_eq!(get(d).depth, 1, "direct dependents at depth 1");
        }
        assert_eq!(get(&deep.id).depth, 2, "second-hop dependent at depth 2");
        // depth=1 prunes the second hop entirely.
        let shallow = eng.impact_depth(&base.id, 1).expect("impact succeeds");
        assert!(
            shallow.nodes.iter().all(|n| n.id != deep.id),
            "depth 1 must exclude the second hop: {:?}",
            shallow.nodes
        );
        // rel filter narrows to USES edges only.
        let filtered = eng
            .impact_filtered(&base.id, 3, Some("USES"))
            .expect("impact succeeds");
        assert!(filtered.nodes.iter().all(|n| n.rel == "USES"));
        assert_eq!(filtered.nodes.len(), 4);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn unknown_id_gives_empty_report() {
        let db = tmp_db("unknown");
        let eng = test_engine(&db);
        let report = eng.impact("no-such-id").expect("impact succeeds");
        assert!(report.nodes.is_empty());
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}
