//! ReMem v3 recall: RRF fusion over FTS + vector (+ optional graph) lists,
//! recency/importance weighting, and a Store+Graph facade.

pub mod rank;
pub mod stub;

pub use rank::{
    apply_floor, final_score, fuse, pack_by_budget, recency_score, rrf, Fused, Ranking,
    DEFAULT_HALF_LIFE_DAYS, DEFAULT_RRF_K,
};
pub use stub::StubEmbedder;

use anyhow::{anyhow, Context, Result};
use remem_graph::Graph;
use remem_store::Store;
use remem_types::{MemoryItem, RecallHit, RecallQuery};
use std::collections::HashSet;

/// Minimal local embedding contract so this crate is not blocked by
/// remem-embed. Vectors must be 768-dim (the store's vec0 width); the real
/// embedder is wired in at integration.
pub trait Embed {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

/// How many top seeds get one graph hop during expansion.
const GRAPH_EXPANSION_SEEDS: usize = 8;

/// Default score floor: 0.0 means off, so recall is unchanged unless a caller
/// opts in (`--min-score`). Calibration on docs/eval-fixtures.json (40 memories,
/// debug binary, Cpu 768d embedder, fresh db): the 3 pure-stopword adversarial
/// queries (`expect: ""`) top out at 0.0129-0.0138; the lowest top-1 score over
/// all 37 answerable queries is 0.023. So any floor in 0.014..0.022 drops all
/// adversarial junk and keeps every answerable top-1 (recall@1 9/37, recall@5
/// 18/37 unchanged at 0.02). Use 0.02 as the round midpoint; it stays off by
/// default because a floor that is wrong for one corpus silently returns [].
pub const DEFAULT_MIN_SCORE: f64 = 0.0;

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
    fn default() -> Self {
        Self {
            fts: 1.0,
            vector: 1.0,
            graph: 1.0,
        }
    }
}

/// Store + Graph + embedder, ranked recall on top.
pub struct RecallEngine {
    store: Store,
    graph: Option<Graph>,
    embed: Box<dyn Embed>,
    weights: Weights,
    half_life_days: f64,
    min_score: f64,
}

impl RecallEngine {
    /// Engine over an open store with no graph projection.
    pub fn new(store: Store, embed: Box<dyn Embed>) -> Self {
        Self {
            store,
            graph: None,
            embed,
            weights: Weights::default(),
            half_life_days: DEFAULT_HALF_LIFE_DAYS,
            min_score: DEFAULT_MIN_SCORE,
        }
    }

    /// Enable graph projection and neighbour expansion on the same db file.
    pub fn with_graph(mut self, graph: Graph) -> Self {
        self.graph = Some(graph);
        self
    }

    pub fn with_weights(mut self, weights: Weights) -> Self {
        self.weights = weights;
        self
    }

    pub fn with_half_life_days(mut self, days: f64) -> Self {
        self.half_life_days = days;
        self
    }

    /// Drop recall hits scoring below `min_score`. `0.0` disables the floor.
    pub fn with_min_score(mut self, min_score: f64) -> Self {
        self.min_score = min_score;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn graph(&self) -> Option<&Graph> {
        self.graph.as_ref()
    }

    /// Embed `texts` in one batch, checking the embedder kept its contract.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let out = self.embed.embed(texts)?;
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

    /// Insert, project onto the graph, and index the embedding.
    pub fn remember(&self, item: &MemoryItem) -> Result<String> {
        let vec = self.embed_one(&item.content)?;
        let id = self.store.insert(item).context("store insert")?;
        self.store
            .set_embedding(&id, &vec)
            .context("set_embedding")?;
        if let Some(g) = &self.graph {
            g.attach(item).map_err(|e| anyhow!("graph attach: {e}"))?;
        }
        Ok(id)
    }

    /// Remove a memory from store and graph.
    pub fn forget(&self, id: &str) -> Result<()> {
        self.store.delete(id).context("store delete")?;
        if let Some(g) = &self.graph {
            g.forget(id).map_err(|e| anyhow!("graph forget: {e}"))?;
        }
        Ok(())
    }

    /// Directed edge between two memories. Nodes must already be attached.
    pub fn link(&self, from: &str, to: &str, rel: Option<&str>) -> Result<()> {
        let g = self
            .graph
            .as_ref()
            .ok_or_else(|| anyhow!("engine has no graph open"))?;
        let rel = rel.unwrap_or(remem_graph::DEFAULT_REL);
        g.link(from, to, rel)
            .map_err(|e| anyhow!("graph link: {e}"))
    }

    pub fn list(&self) -> Result<Vec<MemoryItem>> {
        Ok(self.store.list(false)?)
    }

    /// Combined counts for `remem stats`.
    pub fn stats(&self) -> Result<serde_json::Value> {
        let memories = self.store.list(false)?.len();
        let mut out = serde_json::json!({ "memories": memories });
        if let Some(g) = &self.graph {
            let s = g.stats().map_err(|e| anyhow!("graph stats: {e}"))?;
            out["graph"] = s;
        }
        Ok(out)
    }

    /// Ranked recall over FTS, vector and (optionally) graph expansion.
    pub fn recall(&self, query: &RecallQuery) -> Result<Vec<RecallHit>> {
        let text = query.text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let k = if query.k == 0 { 5 } else { query.k };
        let depth = LIST_DEPTH_FACTOR * k;
        let now = MemoryItem::now();

        // Filtered id set; an unfiltered list would leak soft-scoped hits.
        let allowed: HashSet<String> = self
            .store
            .list(false)?
            .into_iter()
            .filter(|m| matches(m, query))
            .map(|m| m.id)
            .collect();

        let fts: Vec<String> = self
            .store
            .fts_search(text, depth)?
            .into_iter()
            .map(|(m, _bm25)| m.id)
            .filter(|id| allowed.contains(id))
            .collect();

        let qvec = self.embed_one(text)?;
        let vector: Vec<String> = self
            .store
            .knn(&qvec, depth)?
            .into_iter()
            .map(|(id, _dist)| id)
            .filter(|id| allowed.contains(id))
            .collect();

        // Graph expansion: neighbours of the top fts+vector seeds, fused as a
        // third list so linked-but-not-matched memories can surface.
        let graph_ids: Vec<String> = if let Some(g) = &self.graph {
            let seeds = fuse(
                &[Ranking::new("fts", &fts), Ranking::new("vector", &vector)],
                DEFAULT_RRF_K,
            );
            let mut seen = HashSet::new();
            let mut nbrs = Vec::new();
            for seed in seeds.iter().take(k.min(GRAPH_EXPANSION_SEEDS)) {
                for (nid, _rel) in g
                    .neighbors(&seed.id)
                    .map_err(|e| anyhow!("graph neighbors: {e}"))?
                {
                    if allowed.contains(&nid) && seen.insert(nid.clone()) {
                        nbrs.push(nid);
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
            let Some(item) = self.store.get(&entry.id) else {
                continue; // deleted between list and fetch
            };
            // Two clocks: rank by when it happened, not when it was typed.
            let recency = recency_score(item.event_time(), self.half_life_days, now);
            let mut reasons = entry.reasons;
            if recency > 0.9 {
                reasons.push("recent".to_string());
            }
            if item.importance >= 0.8 {
                reasons.push("important".to_string());
            }
            let score = final_score(entry.score, item.importance, recency);
            hits.push(RecallHit {
                item,
                score,
                reasons,
            });
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(k);
        let hits = match query.max_chars {
            Some(max) => pack_by_budget(hits, max),
            None => hits,
        };
        Ok(apply_floor(hits, self.min_score))
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
    let t = item.event_time();
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
