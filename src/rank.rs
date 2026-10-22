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

struct FusedAccum {
    raw_score: f64,
    best_rank: usize,
    ranks: Vec<(String, usize)>,
}

/// Weighted RRF: score = sum_c w_c / (k + rank_c), times
/// (1 + 0.05 * (channels_agreeing - 1)). Sorted by score desc, ties by best
/// rank then id asc. Duplicate ids inside one channel count once (best rank).
pub fn rrf(channels: &[Channel], k: f64) -> Vec<Fused> {
    let mut map: HashMap<i64, FusedAccum> = HashMap::new();

    for ch in channels {
        let mut seen = std::collections::HashSet::new();
        for (idx, &id) in ch.ids.iter().enumerate() {
            if seen.insert(id) {
                let rank = idx + 1;
                let entry = map.entry(id).or_insert_with(|| FusedAccum {
                    raw_score: 0.0,
                    best_rank: rank,
                    ranks: Vec::new(),
                });
                entry.raw_score += ch.weight / (k + rank as f64);
                if rank < entry.best_rank {
                    entry.best_rank = rank;
                }
                entry.ranks.push((ch.name.to_string(), rank));
            }
        }
    }

    let mut result: Vec<(Fused, usize)> = map
        .into_iter()
        .map(|(id, accum)| {
            let channels_agreeing = accum.ranks.len();
            let bonus = 1.0 + 0.05 * (channels_agreeing.saturating_sub(1) as f64);
            let score = accum.raw_score * bonus;
            (
                Fused {
                    id,
                    score,
                    ranks: accum.ranks,
                },
                accum.best_rank,
            )
        })
        .collect();

    result.sort_by(|a, b| {
        b.0.score
            .partial_cmp(&a.0.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });

    result.into_iter().map(|(fused, _)| fused).collect()
}

/// Cosine of two equal-length vectors (0 when either has zero norm).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "cosine vectors must have equal lengths");
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    for (&x, &y) in a.iter().zip(b.iter()) {
        let x = x as f64;
        let y = y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= 0.0 {
        0.0
    } else {
        ((dot / denom) as f32).clamp(-1.0, 1.0)
    }
}

/// Maximal marginal relevance: greedily pick up to k ids from `cands`
/// (id, relevance; relevance min-max normalised internally) maximising
/// lambda * rel - (1 - lambda) * max cosine to already picked. Ids without a
/// vector are treated as dissimilar to everything. Keeps the top candidate first.
pub fn mmr(cands: &[(i64, f64)], vecs: &HashMap<i64, Vec<f32>>, lambda: f64, k: usize) -> Vec<i64> {
    if cands.is_empty() || k == 0 {
        return Vec::new();
    }

    let min_rel = cands.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
    let max_rel = cands.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
    let range = max_rel - min_rel;

    let norm_rels: Vec<f64> = cands
        .iter()
        .map(|c| {
            if range > 0.0 {
                (c.1 - min_rel) / range
            } else {
                1.0
            }
        })
        .collect();

    let mut selected: Vec<i64> = Vec::with_capacity(k.min(cands.len()));
    let mut selected_mask = vec![false; cands.len()];

    // Keeps the top candidate first
    selected.push(cands[0].0);
    selected_mask[0] = true;

    while selected.len() < k && selected.len() < cands.len() {
        let mut best_idx = None;
        let mut best_score = f64::NEG_INFINITY;

        for (i, &(id, _)) in cands.iter().enumerate() {
            if selected_mask[i] {
                continue;
            }

            let max_sim = if let Some(v_i) = vecs.get(&id) {
                let mut sim_max = f64::NEG_INFINITY;
                for &s_id in &selected {
                    let sim = if let Some(v_s) = vecs.get(&s_id) {
                        cosine(v_i, v_s) as f64
                    } else {
                        0.0
                    };
                    if sim > sim_max {
                        sim_max = sim;
                    }
                }
                sim_max
            } else {
                0.0
            };

            let score = lambda * norm_rels[i] - (1.0 - lambda) * max_sim;
            if score > best_score {
                best_score = score;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            selected.push(cands[idx].0);
            selected_mask[idx] = true;
        } else {
            break;
        }
    }

    selected
}

/// Greedy whole-item packing: walk items in order, keep each one whose cost
/// fits the remaining budget; the first item is always kept. Returns kept indices.
pub fn pack(costs: &[usize], budget: usize) -> Vec<usize> {
    if costs.is_empty() {
        return Vec::new();
    }
    let mut kept = Vec::new();
    kept.push(0);
    let mut remaining = budget.saturating_sub(costs[0]);
    for (i, &cost) in costs.iter().enumerate().skip(1) {
        if cost <= remaining {
            kept.push(i);
            remaining -= cost;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_single_channel_order_preserved() {
        let ids = [10, 20, 30];
        let ch = Channel {
            name: "c1",
            weight: 1.0,
            ids: &ids,
        };
        let res = rrf(&[ch], 60.0);
        let out_ids: Vec<i64> = res.iter().map(|f| f.id).collect();
        assert_eq!(out_ids, vec![10, 20, 30]);
    }

    #[test]
    fn test_rrf_two_channels_agreement_bonus() {
        // Channel 1: [10, 20]
        // Channel 2: [20, 30]
        // Id 20 appears in both channels -> channels_agreeing = 2 -> (1 + 0.05 * 1) = 1.05 bonus
        // For id 10: rank 1 in c1, missing in c2. score = (1.0 / (60 + 1)) * 1.0 = 1 / 61 ≈ 0.016393
        // For id 20: rank 2 in c1, rank 1 in c2.
        // base = 1/(60+2) + 1/(60+1) = 1/62 + 1/61 ≈ 0.016129 + 0.016393 = 0.032522
        // score with bonus = 0.032522 * 1.05 ≈ 0.034148
        // id 20 must rank ahead of id 10
        let ids1 = [10, 20];
        let ids2 = [20, 30];
        let channels = [
            Channel {
                name: "c1",
                weight: 1.0,
                ids: &ids1,
            },
            Channel {
                name: "c2",
                weight: 1.0,
                ids: &ids2,
            },
        ];
        let res = rrf(&channels, 60.0);
        assert_eq!(res[0].id, 20);
        assert!((res[0].score - (1.0 / 62.0 + 1.0 / 61.0) * 1.05).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_weights_matter() {
        // Channel 1 has weight 10.0, Channel 2 has weight 1.0
        // c1 has [10], c2 has [20]
        let ids1 = [10];
        let ids2 = [20];
        let channels = [
            Channel {
                name: "heavy",
                weight: 10.0,
                ids: &ids1,
            },
            Channel {
                name: "light",
                weight: 1.0,
                ids: &ids2,
            },
        ];
        let res = rrf(&channels, 60.0);
        assert_eq!(res[0].id, 10);
        assert!(res[0].score > res[1].score);
        assert!((res[0].score - 10.0 / 61.0).abs() < 1e-6);
        assert!((res[1].score - 1.0 / 61.0).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_ranks_reported_1_based() {
        let ids1 = [10, 20];
        let ids2 = [20, 10];
        let channels = [
            Channel {
                name: "bm25",
                weight: 1.0,
                ids: &ids1,
            },
            Channel {
                name: "vec",
                weight: 1.0,
                ids: &ids2,
            },
        ];
        let res = rrf(&channels, 60.0);
        let f10 = res.iter().find(|f| f.id == 10).unwrap();
        assert_eq!(
            f10.ranks,
            vec![("bm25".to_string(), 1), ("vec".to_string(), 2)]
        );
        let f20 = res.iter().find(|f| f.id == 20).unwrap();
        assert_eq!(
            f20.ranks,
            vec![("bm25".to_string(), 2), ("vec".to_string(), 1)]
        );
    }

    #[test]
    fn test_rrf_duplicate_ids_within_channel() {
        // Duplicate id 10 at rank 1 and rank 3 in channel 1
        let ids = [10, 20, 10, 30];
        let ch = Channel {
            name: "c",
            weight: 1.0,
            ids: &ids,
        };
        let res = rrf(&[ch], 60.0);
        assert_eq!(res.len(), 3);
        let f10 = res.iter().find(|f| f.id == 10).unwrap();
        assert_eq!(f10.ranks, vec![("c".to_string(), 1)]);
        assert!((f10.score - 1.0 / 61.0).abs() < 1e-6);
    }

    #[test]
    fn test_rrf_deterministic_tie_break() {
        // Two channels with identical weights and symmetric ranks:
        // c1: [1, 2]
        // c2: [2, 1]
        // Both items have identical scores and identical best ranks (1).
        // Tie breaker should be id asc -> 1 then 2.
        let ids1 = [1, 2];
        let ids2 = [2, 1];
        let channels = [
            Channel {
                name: "c1",
                weight: 1.0,
                ids: &ids1,
            },
            Channel {
                name: "c2",
                weight: 1.0,
                ids: &ids2,
            },
        ];
        let res = rrf(&channels, 60.0);
        assert_eq!(res[0].id, 1);
        assert_eq!(res[1].id, 2);

        // Another case: identical score, but one has a better best rank:
        // Suppose item A has rank 2 in c1 (w=2) -> score = 2/(60+2) = 2/62 ≈ 0.032258
        // Item B has rank 1 in c2 (w=1.93548...) -> but simpler:
        // Say c1 has [10] with weight 1.0 -> score = 1/61, best rank 1
        // c2 has [5, 20] with weight 1.01639344... so scores are equal
        // Item with best rank 1 must precede item with best rank 2.
    }

    #[test]
    fn test_cosine_basics() {
        let v1 = [1.0, 0.0, 0.0];
        let v2 = [0.0, 1.0, 0.0];
        let v3 = [2.0, 0.0, 0.0];
        let v_neg = [-1.0, 0.0, 0.0];
        let v_zero = [0.0, 0.0, 0.0];

        // Orthogonal
        assert!((cosine(&v1, &v2) - 0.0).abs() < 1e-6);
        // Identical direction
        assert!((cosine(&v1, &v3) - 1.0).abs() < 1e-6);
        // Opposite
        assert!((cosine(&v1, &v_neg) - (-1.0)).abs() < 1e-6);
        // Zero norm
        assert_eq!(cosine(&v1, &v_zero), 0.0);
        assert_eq!(cosine(&v_zero, &v_zero), 0.0);
    }

    #[test]
    fn test_mmr_identical_vectors_pushed_down() {
        // Items:
        // id 1: rel 1.0, vec [1.0, 0.0]
        // id 2: rel 0.8, vec [1.0, 0.0] (identical vector to id 1)
        // id 3: rel 0.6, vec [0.0, 1.0] (dissimilar vector)
        let cands = vec![(1, 1.0), (2, 0.8), (3, 0.6)];
        let mut vecs = HashMap::new();
        vecs.insert(1, vec![1.0, 0.0]);
        vecs.insert(2, vec![1.0, 0.0]);
        vecs.insert(3, vec![0.0, 1.0]);

        // With lambda = 0.5, id 3 should be selected before id 2
        let res = mmr(&cands, &vecs, 0.5, 3);
        assert_eq!(res, vec![1, 3, 2]);
    }

    #[test]
    fn test_mmr_lambda_1_reproduces_order() {
        let cands = vec![(1, 1.0), (2, 0.8), (3, 0.6)];
        let mut vecs = HashMap::new();
        vecs.insert(1, vec![1.0, 0.0]);
        vecs.insert(2, vec![1.0, 0.0]);
        vecs.insert(3, vec![0.0, 1.0]);

        let res = mmr(&cands, &vecs, 1.0, 3);
        assert_eq!(res, vec![1, 2, 3]);
    }

    #[test]
    fn test_mmr_k_larger_than_input_returns_all() {
        let cands = vec![(1, 1.0), (2, 0.8), (3, 0.6)];
        let vecs = HashMap::new();

        let res = mmr(&cands, &vecs, 0.5, 10);
        assert_eq!(res.len(), 3);
        assert_eq!(res, vec![1, 2, 3]);
    }

    #[test]
    fn test_pack_first_item_larger_than_budget() {
        let costs = [100, 10, 20];
        let kept = pack(&costs, 50);
        assert_eq!(kept, vec![0]);
    }

    #[test]
    fn test_pack_skip_big_middle_item_keep_later_small() {
        let costs = [10, 50, 10, 20];
        let kept = pack(&costs, 45);
        assert_eq!(kept, vec![0, 2, 3]);
    }

    #[test]
    fn test_pack_zero_budget() {
        let costs = [10, 20];
        let kept = pack(&costs, 0);
        assert_eq!(kept, vec![0]);

        let empty: [usize; 0] = [];
        let kept_empty = pack(&empty, 0);
        assert!(kept_empty.is_empty());
    }
}
