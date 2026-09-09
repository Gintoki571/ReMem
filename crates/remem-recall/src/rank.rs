//! Pure ranking primitives, ported from v2 `src/recall.ts`.
//! No I/O here: everything is testable with plain vectors and numbers.

use std::collections::HashMap;

use remem_types::RecallHit;

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

/// Pack hits to a character budget, keeping rank order. A hit that does not
/// fit the remaining budget is skipped and packing continues with the next
/// one; the top hit is always returned whole so a matching query never comes
/// back empty.
pub fn pack_by_budget(hits: Vec<RecallHit>, max_chars: usize) -> Vec<RecallHit> {
    let mut out: Vec<RecallHit> = Vec::new();
    let mut used = 0usize;
    for hit in hits {
        let len = hit.item.content.len();
        if used + len <= max_chars || out.is_empty() {
            used += len;
            out.push(hit);
        }
    }
    out
}

/// Final ranking score: fused * (0.9 + 0.1 * importance) * (0.9 + 0.1 * recency).
///
/// Both bands are deliberately narrow (docs/multiplier-proposal.md,
/// docs/ranking-study.md). RRF inputs are strong on their own (FTS-alone 33/37
/// and vector-alone 34/37 recall@1 on the eval fixtures) while the fused rank
/// trailed both, because a wide multiplier outvotes a dual `fts#1 + vector#1`
/// agreement. At 0.5..1.0 x 0.7..1.0 the worst case swung a score by 2.14x, far
/// more than the ~10% gap between adjacent ranks, so importance and recency
/// decided the order. Each band now caps its swing at 11%, keeping both signals
/// as tie-breakers that can never override the fusion.
pub fn final_score(fused: f64, importance: f32, recency: f64) -> f64 {
    fused * (0.9 + 0.1 * importance as f64) * (0.9 + 0.1 * recency)
}

/// Multiplier for a hit whose tags share a word with the query: a tie-breaker,
/// not an override, and capped at one factor per hit.
///
/// Small on purpose. With RRF inputs at 89-92% recall@1 alone (docs/ranking-study.md), the
/// job is to lift a tag-only anchor over the few hits fused immediately above it, not to
/// reorder the list. 1.05x measured the same recall@1 as 1.2x on the eval fixtures and lost
/// nothing at recall@5, so there is no reason to go wider. Beware tuning offline: re-ranking
/// `--k 40` output predicted a different winner than the real k=5 runner, because list depth
/// is 4k and the candidate set changes with k.
pub const TAG_MATCH_BOOST: f64 = 1.05;

/// Leading characters a query word and a tag must agree on to count as a
/// match. Measured on docs/eval-fixtures.json: 3 pulls noise (`per`/`perf`,
/// `second`/`security`, `production`/`process`), 5 loses the stem drift the
/// channel exists for (`decide`/`decision`). 4 is the only width that gets
/// both right.
pub const TAG_MIN_PREFIX: usize = 4;

/// Split on non-alphanumerics, lowercased. Manual scan so this crate does not
/// need a regex dependency.
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

// ponytail: local copy of remem-store's private STOPWORDS (not importable),
// plus "thing" (adversarial-battery #9: "what is the thing" is content-free).
// Keep in sync with crates/remem-store/src/lib.rs STOPWORDS by eye.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "did", "do", "does", "for", "from",
    "had", "has", "have", "how", "i", "in", "is", "it", "no", "not", "of", "on", "or", "that",
    "the", "thing", "this", "to", "was", "what", "when", "where", "which", "who", "will", "with",
];

/// True when no content-bearing token remains after stopword removal.
/// Empty input counts as content-free.
pub fn is_content_free(query: &str) -> bool {
    let toks = tokens(query);
    toks.is_empty() || toks.iter().all(|t| STOPWORDS.contains(&t.as_str()))
}

/// Lexical overlap between query words and hit tags.
///
/// One `(id, factor)` per input hit, in input order: `TAG_MATCH_BOOST` when
/// any of the hit's tags overlaps a query word, else `1.0`.
///
/// Deliberately not driven by `RecallQuery::tags`: that filter already dropped
/// non-matching rows, so every surviving hit shares those tags and the filter
/// carries no ranking signal. The useful signal is overlap between words in the
/// free-text query and a hit's tags, which is what Q27 needs ("decide" vs tag
/// `decision`) - a link that exists only in tags, so neither FTS (content) nor
/// the embedder separates it.
///
/// Matching is case-insensitive on both sides and keyed on shared prefix
/// length: a word and a tag match when they agree on the first
/// [`TAG_MIN_PREFIX`] characters. That covers plural/stem drift
/// (`decide`/`decision`, `backups`/`backup`) without a stemmer, which exact word
/// equality would miss. Words and tags shorter than the prefix length cannot
/// match, so the 3-letter tags (`api`, `db`, `ui`) and function words are
/// excluded for free. The boost is capped at one factor per hit however many
/// words match, so a vague query cannot stack it; drift can still over-match
/// (`worker`/`workflow`), bounded at [`TAG_MATCH_BOOST`].
pub fn tag_boost(hits: &[(&str, &[String])], query_words: &[String]) -> Vec<(String, f64)> {
    hits.iter()
        .map(|(id, tags)| {
            let shared = query_words.iter().any(|w| {
                let w = prefix(w);
                tags.iter().any(|t| {
                    let t = prefix(t);
                    t.len() >= TAG_MIN_PREFIX && w.len() >= TAG_MIN_PREFIX && t == w
                })
            });
            let factor = if shared { TAG_MATCH_BOOST } else { 1.0 };
            ((*id).to_string(), factor)
        })
        .collect()
}

/// Lowercased first [`TAG_MIN_PREFIX`] characters. Char-based, so a multibyte
/// tag cannot be sliced mid-codepoint, and case-folded on both sides so a
/// `Decision` tag matches a `decide` query.
fn prefix(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .take(TAG_MIN_PREFIX)
        .collect()
}

/// Drop hits whose final score is below `min_score`. Unlike budget packing
/// this may return an empty list: when even the top hit is junk, the right
/// answer is no results. `min_score <= 0.0` disables the floor (scores are
/// always >= 0), so 0.0 means off.
pub fn apply_floor(hits: Vec<RecallHit>, min_score: f64) -> Vec<RecallHit> {
    hits.into_iter().filter(|h| h.score >= min_score).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use remem_types::{MemoryItem, MemoryKind};

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

    fn hit(id: &str, content: &str) -> RecallHit {
        let mut item = MemoryItem::new(MemoryKind::Fact, content.to_string());
        item.id = id.to_string();
        RecallHit {
            item,
            score: 1.0,
            reasons: vec![],
        }
    }

    fn packed(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn budget_ids(hits: &[RecallHit]) -> Vec<String> {
        hits.iter().map(|h| h.item.id.clone()).collect()
    }

    #[test]
    fn pack_keeps_all_when_budget_is_generous() {
        let hits = vec![hit("a", "aaaa"), hit("b", "bbbb")];
        assert_eq!(budget_ids(&pack_by_budget(hits, 100)), packed(&["a", "b"]));
    }

    #[test]
    fn pack_skips_what_does_not_fit_and_continues() {
        // rank order a,b,c; the long b is skipped but short c still fits.
        let hits = vec![hit("a", "aaaa"), hit("b", &"b".repeat(50)), hit("c", "cc")];
        assert_eq!(budget_ids(&pack_by_budget(hits, 10)), packed(&["a", "c"]));
    }

    #[test]
    fn pack_never_returns_empty_when_there_is_a_top_hit() {
        let hits = vec![hit("a", &"a".repeat(100)), hit("b", &"b".repeat(100))];
        assert_eq!(budget_ids(&pack_by_budget(hits.clone(), 1)), packed(&["a"]));
        assert_eq!(budget_ids(&pack_by_budget(hits, 0)), packed(&["a"]));
    }

    #[test]
    fn pack_respects_exact_budget_and_empty_input() {
        let hits = vec![hit("a", "aaaa"), hit("b", "bbbb")];
        let big = pack_by_budget(hits.clone(), 8);
        let exact = pack_by_budget(hits, 7);
        assert_eq!(budget_ids(&big), packed(&["a", "b"]));
        assert_eq!(budget_ids(&exact), packed(&["a"]));
        assert!(pack_by_budget(Vec::new(), 10).is_empty());
    }

    fn scored(id: &str, score: f64) -> RecallHit {
        let mut h = hit(id, "content");
        h.score = score;
        h
    }

    fn floored_ids(hits: &[RecallHit]) -> Vec<String> {
        hits.iter().map(|h| h.item.id.clone()).collect()
    }

    #[test]
    fn floor_drops_only_hits_below_it() {
        let hits = vec![scored("a", 0.03), scored("b", 0.02), scored("c", 0.01)];
        assert_eq!(floored_ids(&apply_floor(hits, 0.02)), packed(&["a", "b"]));
    }

    #[test]
    fn floor_zero_is_off_and_keeps_everything() {
        let hits = vec![scored("a", 0.0), scored("b", 0.5)];
        assert_eq!(floored_ids(&apply_floor(hits, 0.0)), packed(&["a", "b"]));
    }

    #[test]
    fn floor_above_top_hit_returns_empty_even_for_the_top() {
        let hits = vec![scored("a", 0.013), scored("b", 0.01)];
        assert!(apply_floor(hits, 0.02).is_empty());
    }

    #[test]
    fn floor_on_empty_input_is_empty() {
        assert!(apply_floor(Vec::new(), 0.5).is_empty());
    }

    fn tg(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| t.to_string()).collect()
    }

    fn boost_of(out: &[(String, f64)], id: &str) -> f64 {
        out.iter().find(|(i, _)| i == id).unwrap().1
    }

    #[test]
    fn tag_boost_matches_stems_and_case() {
        let (a, b) = (tg(&["decision", "process"]), tg(&["storage"]));
        let hits: Vec<(&str, &[String])> = vec![("t", &a), ("o", &b)];
        let out = tag_boost(&hits, &tokens("What did we DECIDE about spending"));
        assert_eq!(boost_of(&out, "t"), TAG_MATCH_BOOST); // decide ~ decision
        assert_eq!(boost_of(&out, "o"), 1.0);
        assert_eq!(
            out.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>(),
            vec!["t", "o"] // input order preserved
        );
    }

    #[test]
    fn tag_boost_ignores_short_words_and_tags() {
        let (a, b) = (tg(&["api"]), tg(&["decision"]));
        let hits: Vec<(&str, &[String])> = vec![("a", &a), ("b", &b)];
        // "api" is a 3-char tag and "db"/"ui" 3-char words: no match either way.
        let out = tag_boost(&hits, &tokens("db ui api decide"));
        assert_eq!(boost_of(&out, "a"), 1.0);
        assert_eq!(boost_of(&out, "b"), TAG_MATCH_BOOST);
        assert!(tag_boost(&[], &tokens("decision")).is_empty());
    }

    #[test]
    fn tag_boost_folds_case_on_both_sides() {
        let a = tg(&["Decision"]);
        let hits: Vec<(&str, &[String])> = vec![("a", &a)];
        // Tag folded too, not just the query word.
        let out = tag_boost(&hits, &tokens("what did we decide"));
        assert_eq!(boost_of(&out, "a"), TAG_MATCH_BOOST);
    }

    #[test]
    fn tag_boost_survives_multibyte_tags() {
        // Byte-slicing at 4 would panic mid-codepoint on these: `日本` is 2 chars
        // but 6 bytes, so the old length check passed and a[..4] split a char.
        let (a, b, c) = (tg(&["Überblick"]), tg(&["日本語タグ"]), tg(&["日本"]));
        let hits: Vec<(&str, &[String])> = vec![("a", &a), ("b", &b), ("c", &c)];
        let out = tag_boost(&hits, &tokens("überblick 日本語タ 日本語"));
        assert_eq!(boost_of(&out, "a"), TAG_MATCH_BOOST); // case-folded, non-ASCII
        assert_eq!(boost_of(&out, "b"), TAG_MATCH_BOOST); // char-based prefix
        assert_eq!(boost_of(&out, "c"), 1.0); // 2-char tag cannot reach 4 chars
    }

    #[test]
    fn tag_boost_is_bounded_and_capped_per_hit() {
        let a = tg(&["backup", "testing", "deploy"]);
        let hits: Vec<(&str, &[String])> = vec![("a", &a)];
        // Three matching words still give one factor, not boost^3.
        let out = tag_boost(&hits, &tokens("backups testing deploy now"));
        assert_eq!(boost_of(&out, "a"), TAG_MATCH_BOOST);
        assert!((1.0..=1.3).contains(&boost_of(&out, "a")), "bounded band");
    }

    #[test]
    fn final_score_formula() {
        let s = final_score(1.0, 0.5, 1.0);
        assert!((s - 0.95 * 1.0).abs() < 1e-12);
        let s = final_score(1.0, 0.9, 0.0);
        assert!((s - 0.99 * 0.9).abs() < 1e-6); // importance goes through f32
                                                // A dual fts#1+vector#1 (2/61) must beat an importance-1.0 single-list
                                                // hit (1/61): the old 0.5..1.0 band inverted exactly this.
        let dual = final_score(2.0 / 61.0, 0.0, 0.0);
        let solo = final_score(1.0 / 61.0, 1.0, 1.0);
        assert!(dual > solo, "multipliers must not override fusion");
    }

    #[test]
    fn content_free_stopword_only_and_thing() {
        assert!(is_content_free("what is the thing"));
        assert!(is_content_free("the"));
        assert!(is_content_free("   "));
        assert!(!is_content_free("redis sharding"));
    }
}
