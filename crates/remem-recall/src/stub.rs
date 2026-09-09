//! Deterministic offline embedder used for CLI wiring until remem-embed is
//! integrated: hashed character n-grams projected into 768 dims, L2 normalized.
//! Lexical, not semantic: FTS carries keyword hits, this gives the vector list
//! a stable "similar words -> similar vector" signal.

use crate::Embed;

/// Vector width required by the store's vec0 table.
pub const DIMS: usize = 768;

pub struct StubEmbedder;

impl Embed for StubEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| embed_one(t)).collect())
    }
}

fn embed_one(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; DIMS];
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    for w in &words {
        add_feature(&mut v, w.as_bytes());
        // character trigrams inside a word catch rough morphological overlap
        let b = w.as_bytes();
        for i in 0..b.len().saturating_sub(2) {
            add_feature(&mut v, &b[i..i + 3]);
        }
    }
    // whole-string trigrams so reordered phrases still share mass
    let bytes = lower.as_bytes();
    for i in 0..bytes.len().saturating_sub(2) {
        add_feature(&mut v, &bytes[i..i + 3]);
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

fn add_feature(v: &mut [f32], feat: &[u8]) {
    // FNV-1a: dependency-free, stable across builds.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in feat {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let idx = (h % v.len() as u64) as usize;
    // sign bit spreads collisions instead of always adding
    let sign = if h >> 63 == 1 { 1.0 } else { -1.0 };
    v[idx] += sign;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn normalized_and_sized() {
        let v = embed_one("the quick brown fox jumps");
        assert_eq!(v.len(), DIMS);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn similar_text_scores_higher_than_random() {
        let q = embed_one("deploy the rust binary with cargo");
        let close = embed_one("deploy the rust binary using cargo release");
        let far = embed_one("banana smoothie recipe for brunch");
        assert!(dot(&q, &close) > dot(&q, &far));
    }

    #[test]
    fn empty_text_is_zero_vector() {
        let v = embed_one("");
        assert!(v.iter().all(|x| *x == 0.0));
    }
}
