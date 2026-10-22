//! Embeddings: the `Embedder` trait, the UltraBERT implementation, and a
//! deterministic `HashEmbedder` for tests.

use crate::error::Result;
use std::path::Path;

pub const DIMS: usize = 768;
/// Prompt prepended to queries (config_sentence_transformers.json "query").
pub const QUERY_PREFIX: &str =
    "Instruct: Given a question, retrieve passages that answer the question\nQuery: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Search query: QUERY_PREFIX is prepended.
    Query,
    /// Stored text (memories, entity cards, facts, predicates): no prefix.
    Document,
}

pub trait Embedder: Send + Sync {
    /// One L2-normalised DIMS-long vector per input, same order.
    fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>>;
    /// Short label for stats ("ultrabert-150m", "hash").
    fn name(&self) -> &str;
}

/// Deterministic bag of word unigrams + char trigrams hashed into DIMS
/// buckets, L2-normalised. Only for tests and as an explicit fallback.
pub struct HashEmbedder;

impl Embedder for HashEmbedder {
    fn embed(&self, texts: &[&str], _mode: Mode) -> Result<Vec<Vec<f32>>> {
        todo!("lane embed")
    }
    fn name(&self) -> &str {
        "hash"
    }
}

/// Load UltraBERT from `dir` (or NEOCORTEX_MODEL_DIR, or
/// ~/.neocortex/models/UltraBERT-Embed-150M). Errors if the dir is missing.
pub fn load(dir: Option<&Path>) -> Result<Box<dyn Embedder>> {
    todo!("lane embed")
}
