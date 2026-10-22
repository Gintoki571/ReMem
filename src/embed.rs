//! Embeddings: the `Embedder` trait, the UltraBERT implementation, and a
//! deterministic `HashEmbedder` for tests.

use crate::error::Result;
use crate::ultrabert::UltraBert;
use candle_core::{DType, Device, Tensor};
use std::fs;
use std::path::Path;
use tokenizers::{tokenizer::TruncationDirection, Tokenizer};

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
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0f32; DIMS];
                for word in text.split_whitespace() {
                    add_hash(&mut vector, word.as_bytes());
                }
                let chars: Vec<char> = text.to_lowercase().chars().collect();
                for window in chars.windows(3) {
                    let mut bytes = [0u8; 12];
                    let mut n = 0;
                    for c in window {
                        for byte in c.to_string().as_bytes() {
                            if n < bytes.len() {
                                bytes[n] = *byte;
                                n += 1;
                            }
                        }
                    }
                    add_hash(&mut vector, &bytes[..n]);
                }
                normalise(&mut vector);
                vector
            })
            .collect())
    }
    fn name(&self) -> &str {
        "hash"
    }
}

/// Load UltraBERT from `dir` (or NEOCORTEX_MODEL_DIR, or
/// ~/.neocortex/models/UltraBERT-Embed-150M). Errors if the dir is missing.
pub fn load(dir: Option<&Path>) -> Result<Box<dyn Embedder>> {
    let path = dir
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("NEOCORTEX_MODEL_DIR").map(|p| Path::new(&p).to_path_buf()))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|p| Path::new(&p).join(".neocortex/models/UltraBERT-Embed-150M"))
        })
        .unwrap_or_else(|| Path::new(".neocortex/models/UltraBERT-Embed-150M").to_path_buf());
    if !path.is_dir() {
        return Err(crate::err!(
            "UltraBERT model directory is missing: {}",
            path.display()
        ));
    }
    let tokenizer = Tokenizer::from_file(path.join("tokenizer.json"))?;
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path.join("config.json"))?)?;
    let model = UltraBert::from_files(&path.join("model.safetensors"), &config)?;
    Ok(Box::new(UltraEmbedder { tokenizer, model }))
}

fn add_hash(vector: &mut [f32], bytes: &[u8]) {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    vector[(hash as usize) % DIMS] += 1.0;
}

fn normalise(vector: &mut [f32]) {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

struct UltraEmbedder {
    tokenizer: Tokenizer,
    model: UltraBert,
}

impl Embedder for UltraEmbedder {
    fn embed(&self, texts: &[&str], mode: Mode) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let max_tokens = std::env::var("NEOCORTEX_MAX_TOKENS")
            .ok()
            .and_then(|x| x.parse().ok())
            .unwrap_or(1024usize);
        let mut encoded = Vec::with_capacity(texts.len());
        for text in texts {
            let text = if mode == Mode::Query {
                format!("{QUERY_PREFIX}{text}")
            } else {
                (*text).to_owned()
            };
            let mut encoding = self.tokenizer.encode(text, true)?;
            if encoding.len() > max_tokens {
                encoding.truncate(max_tokens, 0, TruncationDirection::Right);
            }
            encoded.push(encoding);
        }
        let mut order: Vec<usize> = (0..encoded.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(encoded[i].len()));
        let max_len = order.iter().map(|&i| encoded[i].len()).max().unwrap_or(0);
        let mut ids = vec![0u32; order.len() * max_len];
        let mut lengths = Vec::with_capacity(order.len());
        for (row, &index) in order.iter().enumerate() {
            let row_ids = encoded[index].get_ids();
            lengths.push(row_ids.len());
            ids[row * max_len..row * max_len + row_ids.len()].copy_from_slice(row_ids);
        }
        let ids = Tensor::from_vec(ids, (order.len(), max_len), &Device::Cpu)?;
        let hidden = self.model.forward(&ids, &lengths)?;
        let pooled = hidden.narrow(1, 0, 1)?.squeeze(1)?;
        let norms = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
        let result = pooled
            .broadcast_div(&norms)?
            .to_dtype(DType::F32)?
            .to_vec2::<f32>()?;
        let mut output = vec![Vec::new(); texts.len()];
        for (row, &index) in order.iter().enumerate() {
            output[index] = result[row].clone();
        }
        Ok(output)
    }
    fn name(&self) -> &str {
        "ultrabert-150m"
    }
}
