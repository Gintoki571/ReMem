use neocortex::embed::{Embedder, HashEmbedder, Mode, DIMS, QUERY_PREFIX};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tokenizers::Tokenizer;

#[derive(Deserialize)]
struct Fixture {
    query_prefix: String,
    docs: Vec<Entry>,
    queries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    text: String,
    ids: Vec<u32>,
    emb: Vec<f32>,
}

fn fixture() -> Fixture {
    serde_json::from_str(&fs::read_to_string("tests/fixtures/ultrabert_fixture.json").unwrap())
        .unwrap()
}

fn model_dir() -> PathBuf {
    std::env::var_os("NEOCORTEX_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut p = dirs_fallback();
            p.push(".neocortex/models/UltraBERT-Embed-150M");
            p
        })
}

fn dirs_fallback() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    dot / (aa.sqrt() * bb.sqrt())
}

#[test]
fn hash_embeddings_are_stable_and_semantically_local() {
    let e = HashEmbedder;
    let out = e
        .embed(
            &["the red fox", "the red fox jumps", "database schema"],
            Mode::Document,
        )
        .unwrap();
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|v| v.len() == DIMS));
    assert!(cosine(&out[0], &out[1]) > cosine(&out[0], &out[2]));
    assert!((out[0].iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-5);
}

#[test]
fn tokenizer_matches_reference_ids() {
    let dir = model_dir();
    if !dir.is_dir() {
        println!(
            "skipping UltraBERT parity: model directory absent: {}",
            dir.display()
        );
        return;
    }
    let f = fixture();
    assert_eq!(f.query_prefix, QUERY_PREFIX);
    let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json")).unwrap();
    for entry in &f.docs {
        let ids = tokenizer
            .encode(entry.text.as_str(), true)
            .unwrap()
            .get_ids()
            .to_vec();
        assert_eq!(ids, entry.ids);
    }
    for entry in &f.queries {
        let text = format!("{QUERY_PREFIX}{}", entry.text);
        let ids = tokenizer.encode(text, true).unwrap().get_ids().to_vec();
        assert_eq!(ids, entry.ids);
    }
}

#[test]
fn ultrabert_embeddings_match_reference_and_batching() {
    let dir = model_dir();
    if !dir.is_dir() {
        println!(
            "skipping UltraBERT parity: model directory absent: {}",
            dir.display()
        );
        return;
    }
    let f = fixture();
    let model = neocortex::embed::load(Some(&dir)).unwrap();
    let docs: Vec<&str> = f.docs.iter().map(|x| x.text.as_str()).collect();
    let got_docs = model.embed(&docs, Mode::Document).unwrap();
    for (got, expected) in got_docs.iter().zip(&f.docs) {
        assert!(cosine(got, &expected.emb) >= 0.999);
    }
    let queries: Vec<&str> = f.queries.iter().map(|x| x.text.as_str()).collect();
    let got_queries = model.embed(&queries, Mode::Query).unwrap();
    for (got, expected) in got_queries.iter().zip(&f.queries) {
        assert!(cosine(got, &expected.emb) >= 0.999);
    }
    let one = model.embed(&[docs[4]], Mode::Document).unwrap();
    assert!(cosine(&one[0], &got_docs[4]) >= 0.9999);
}
