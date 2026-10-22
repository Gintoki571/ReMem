//! Pure ranking helpers: weighted RRF fusion, MMR diversification, cosine,
//! budget packing.

use std::collections::HashMap;

/// One channel's ranked ids (best first) and its fusion weight.
pub struct Channel<'a> {
    pub name: &'a str,
    pub weight: f64,
    pub ids: &'a [i64],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fused {
    pub id: i64,
    pub score: f64,
    /// (channel name, 1-based rank) for every channel that returned the id.
    pub ranks: Vec<(String, usize)>,
}

/// Weighted RRF: score = sum_c w_c / (k + rank_c), times
/// (1 + 0.05 * (channels_agreeing - 1)). Sorted by score desc, ties by best
/// rank then id asc. Duplicate ids inside one channel count once (best rank).
pub fn rrf(channels: &[Channel], k: f64) -> Vec<Fused> {
    todo!("lane graph")
}

/// Cosine of two equal-length vectors (0 when either has zero norm).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    todo!("lane graph")
}

/// Maximal marginal relevance: greedily pick up to k ids from `cands`
/// (id, relevance; relevance min-max normalised internally) maximising
/// lambda * rel - (1 - lambda) * max cosine to already picked. Ids without a
/// vector are treated as dissimilar to everything. Keeps the top candidate first.
pub fn mmr(cands: &[(i64, f64)], vecs: &HashMap<i64, Vec<f32>>, lambda: f64, k: usize) -> Vec<i64> {
    todo!("lane graph")
}

/// Greedy whole-item packing: walk items in order, keep each one whose cost
/// fits the remaining budget; the first item is always kept. Returns kept indices.
pub fn pack(costs: &[usize], budget: usize) -> Vec<usize> {
    todo!("lane graph")
}
