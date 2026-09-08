//! Pure ranking primitives, ported from v2 `src/recall.ts`.
//! No I/O here: everything is testable with plain vectors and numbers.

use std::collections::HashMap;

/// Reciprocal rank fusion constant (Cormack et al.), same as v2.
pub const DEFAULT_RRF_K: usize = 60;
/// Recency half-life in days, same as v2.
pub const DEFAULT_HALF_LIFE_DAYS: f64 = 30.0;

/// One ranked id list contributing to a fusion. `ids` are best-first.
#[derive(Debug, Clone, Copy)]
pub struct Ranking<'a> {
    /// Label used in hit reasons, e.g. "fts", "vector".
    pub name: &'a str,
    pub ids: &'a [String],
    pub weight: f64,
}

impl<'a> Ranking<'a> {
    pub fn new(name: &'a str, ids: &'a [String]) -> Self {
        Self {
            name,
            ids,
            weight: 1.0,
        }
    }
}

/// A fused entry: summed weighted RRF score plus "name#rank" reasons.
#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    pub id: String,
    pub score: f64,
    pub reasons: Vec<String>,
}

/// RRF contribution of a zero-based rank: 1 / (k + rank + 1).
pub fn rrf(rank: usize, k: usize) -> f64 {
    1.0 / ((k + rank + 1) as f64)
}

/// Exponential decay, 1.0 at age 0, 0.5 after one half-life. Future
/// timestamps clamp to 0 age.
pub fn recency_score(updated_at: i64, half_life_days: f64, now: i64) -> f64 {
    let age_days = (now - updated_at).max(0) as f64 / 86_400.0;
    0.5f64.powf(age_days / half_life_days)
}

/// Fuse any number of ranked id lists by summing weighted RRF scores.
/// Output is sorted by score descending; ties keep first-seen order
/// (stable sort over list order), so results are deterministic.
pub fn fuse(lists: &[Ranking], k: usize) -> Vec<Fused> {
    let mut acc: HashMap<&str, (f64, Vec<String>)> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for list in lists {
        for (rank, id) in list.ids.iter().enumerate() {
            let entry = acc.entry(id).or_insert_with(|| {
                order.push(id.as_str());
                (0.0, Vec::new())
            });
            entry.0 += list.weight * rrf(rank, k);
            entry.1.push(format!("{}#{}", list.name, rank + 1));
        }
    }
    let mut out: Vec<Fused> = order
        .into_iter()
        .map(|id| {
            let (score, reasons) = acc.remove(id).expect("key present");
            Fused {
                id: id.to_string(),
                score,
                reasons,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Final ranking score: fused * (0.5 + 0.5 * importance) * (0.7 + 0.3 * recency).
pub fn final_score(fused: f64, importance: f32, recency: f64) -> f64 {
    fused * (0.5 + 0.5 * importance as f64) * (0.7 + 0.3 * recency)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rrf_values() {
        assert_eq!(rrf(0, 60), 1.0 / 61.0);
        assert_eq!(rrf(1, 60), 1.0 / 62.0);
    }

    #[test]
    fn fuse_single_list_keeps_order() {
        let a = ids(&["x", "y", "z"]);
        let out = fuse(&[Ranking::new("fts", &a)], 60);
        assert_eq!(
            out.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["x", "y", "z"]
        );
        assert_eq!(out[0].reasons, vec!["fts#1"]);
    }

    #[test]
    fn fuse_agrees_across_lists_ranks_first() {
        let fts = ids(&["a", "b"]);
        let vec = ids(&["a", "c"]);
        let out = fuse(
            &[Ranking::new("fts", &fts), Ranking::new("vector", &vec)],
            60,
        );
        assert_eq!(out[0].id, "a");
        assert!((out[0].score - 2.0 / 61.0).abs() < 1e-12);
        assert_eq!(out[0].reasons, vec!["fts#1", "vector#1"]);
    }

    #[test]
    fn fuse_disagreement_second_beats_first_of_one_list() {
        let fts = ids(&["only-fts", "both"]);
        let vec = ids(&["both"]);
        let out = fuse(
            &[Ranking::new("fts", &fts), Ranking::new("vector", &vec)],
            60,
        );
        assert_eq!(out[0].id, "both");
    }

    #[test]
    fn fuse_weight_scales_contribution() {
        let a = ids(&["x"]);
        let out = fuse(
            &[Ranking {
                name: "v",
                ids: &a,
                weight: 2.0,
            }],
            60,
        );
        assert!((out[0].score - 2.0 / 61.0).abs() < 1e-12);
    }

    #[test]
    fn fuse_empty_lists_is_empty() {
        let e: Vec<String> = Vec::new();
        assert!(fuse(&[Ranking::new("fts", &e)], 60).is_empty());
    }

    #[test]
    fn recency_halves_per_half_life() {
        let now = 1_700_000_000;
        let half_life = 30.0;
        assert!((recency_score(now, half_life, now) - 1.0).abs() < 1e-12);
        let thirty_days = 30 * 86_400;
        let s = recency_score(now - thirty_days, half_life, now);
        assert!((s - 0.5).abs() < 1e-9);
        let sixty_days = 60 * 86_400;
        let s = recency_score(now - sixty_days, half_life, now);
        assert!((s - 0.25).abs() < 1e-9);
    }

    #[test]
    fn recency_clamps_future_dates() {
        assert!((recency_score(200, 30.0, 100) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn final_score_formula() {
        let s = final_score(1.0, 0.5, 1.0);
        assert!((s - 1.0 * 0.75 * 1.0).abs() < 1e-12);
        let s = final_score(1.0, 0.9, 0.0);
        assert!((s - 0.95 * 0.7).abs() < 1e-6); // importance goes through f32
    }
}
