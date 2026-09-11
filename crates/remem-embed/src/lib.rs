//! Local embedding runtime: BERT mean-pool + L2 norm (matches v2 behavior).
//! Default: candle CPU forward. With the `cuda` feature, loads prefer the cudarc
//! GPU backend (`cuda.rs`, sm_75 / CUDA 13.x — DEPRECATED stopgap, see docs/cuda.md)
//! and fall back to CPU on any CUDA error.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};

/// Model dir default; override with `REMEM_EMBED_MODEL_DIR`.
pub const DEFAULT_MODEL_DIR: &str = "/home/bindesh/rag/cadet-embed-base-v1";
/// Truncate inputs to BERT's position limit.
pub const MAX_LEN: usize = 512;

/// GPU backend type; unit placeholder when the `cuda` feature is off.
#[cfg(feature = "cuda")]
type GpuBackend = crate::cuda::CudaEmbedder;
#[cfg(not(feature = "cuda"))]
type GpuBackend = ();

pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    fn dims(&self) -> usize;
}

pub struct LocalEmbedder {
    /// `None` on the GPU path (cudarc embedder owns the weights instead).
    model: Option<BertModel>,
    tokenizer: tokenizers::Tokenizer,
    device: Device,
    dims: usize,
    /// GPU (cudarc) embedder when the `cuda` feature compiled and the device works;
    /// `None` = candle CPU forward. DEPRECATED with the cuda backend (see cuda.rs).
    gpu: Option<GpuBackend>,
}

impl Embedder for LocalEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        #[cfg(feature = "cuda")]
        if let Some(g) = &self.gpu {
            return g.embed(texts);
        }
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let encs = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        let width = encs
            .iter()
            .map(|e| e.len())
            .max()
            .unwrap_or(0)
            .clamp(1, MAX_LEN);
        let n = texts.len();
        let mut ids = Vec::with_capacity(n * width);
        let mut mask = Vec::with_capacity(n * width);
        for e in &encs {
            let take = e.len().min(width);
            ids.extend_from_slice(&e.get_ids()[..take]);
            mask.extend_from_slice(&e.get_attention_mask()[..take]);
            ids.extend(std::iter::repeat_n(0u32, width - take));
            mask.extend(std::iter::repeat_n(0u32, width - take));
        }
        let input_ids = Tensor::new(ids, &self.device)?.reshape((n, width))?;
        let attn = Tensor::new(mask, &self.device)?.reshape((n, width))?;
        let type_ids = Tensor::zeros((n, width), DType::U32, &self.device)?;
        let hidden = self
            .model
            .as_ref()
            .expect("cpu path always has a candle model")
            .forward(&input_ids, &type_ids, Some(&attn))?;
        // Mean-pool over real tokens in plain Rust (avoids broadcast-shape
        // surprises), then L2-normalize.
        let h3: Vec<Vec<Vec<f32>>> = hidden.to_vec3()?;
        let m2: Vec<Vec<u32>> = attn.to_vec2()?;
        let dim = h3[0][0].len();
        let mut out = Vec::with_capacity(n);
        for (h, m) in h3.iter().zip(m2.iter()) {
            let mut acc = vec![0.0f32; dim];
            let mut count = 0.0f32;
            for (tok, &on) in h.iter().zip(m.iter()) {
                if on == 0 {
                    continue;
                }
                for (a, &x) in acc.iter_mut().zip(tok.iter()) {
                    *a += x;
                }
                count += 1.0;
            }
            let count = count.max(1.0);
            for a in acc.iter_mut() {
                *a /= count;
            }
            let norm: f32 = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            for a in acc.iter_mut() {
                *a /= norm;
            }
            out.push(acc);
        }
        Ok(out)
    }

    fn dims(&self) -> usize {
        self.dims
    }
}

impl LocalEmbedder {
    pub fn device_name(&self) -> String {
        #[cfg(feature = "cuda")]
        if let Some(g) = &self.gpu {
            return g.device_name();
        }
        format!("{:?}", self.device)
    }

    #[cfg(feature = "cuda")]
    fn from_gpu(g: crate::cuda::CudaEmbedder) -> Self {
        let dims = g.dims();
        Self {
            model: None,
            tokenizer: g.tokenizer_clone(),
            device: Device::Cpu,
            dims,
            gpu: Some(g),
        }
    }
}

pub fn model_dir() -> PathBuf {
    std::env::var("REMEM_EMBED_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_MODEL_DIR))
}

/// GPU first, CPU fallback. Never fails on device selection itself.
pub fn load() -> Result<LocalEmbedder> {
    load_from(&model_dir())
}

/// Build the CUDA embedder when the `cuda` feature is on; CPU candle otherwise.
#[cfg(feature = "cuda")]
fn try_cuda(dir: &Path) -> Option<LocalEmbedder> {
    match cuda::CudaEmbedder::load(dir) {
        Ok(g) => Some(LocalEmbedder::from_gpu(g)),
        Err(e) => {
            eprintln!("cuda embedder unavailable, CPU fallback: {e:#}");
            None
        }
    }
}
#[cfg(not(feature = "cuda"))]
fn try_cuda(_dir: &Path) -> Option<LocalEmbedder> {
    None
}

/// Load weights from `dir` (`config.json`, `model.safetensors`, `tokenizer.json`).
///
/// Trust boundary: `dir` (default [`DEFAULT_MODEL_DIR`], override
/// `REMEM_EMBED_MODEL_DIR`) is trusted local input. The weights file is
/// memory-mapped with no checksum verification; point it only at model files
/// you trust. Checksum/pinned-hash verification is future work (no code yet).
pub fn load_from(dir: &Path) -> Result<LocalEmbedder> {
    let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);
    load_from_with_device(dir, device)
}

/// Same trust boundary as [`load_from`]: `dir` is trusted input whose
/// `model.safetensors` is mmaped read-only without integrity checking
/// (checksum verification is future work).
fn load_from_with_device(dir: &Path, device: Device) -> Result<LocalEmbedder> {
    if let Some(g) = try_cuda(dir) {
        return Ok(g);
    }
    load_candle(dir, device)
}

/// Direct candle load, bypassing the cudarc backend (CPU reference path).
pub fn load_candle(dir: &Path, device: Device) -> Result<LocalEmbedder> {
    let config: Config = serde_json::from_str(
        &std::fs::read_to_string(dir.join("config.json")).context("read config.json")?,
    )
    .context("parse config.json")?;
    let weights = dir.join("model.safetensors");
    // Safe: file is trusted local weights, mapped read-only.
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)? };
    let model = BertModel::load(vb, &config).context("load bert weights")?;
    let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("load tokenizer.json: {e}"))?;
    Ok(LocalEmbedder {
        model: Some(model),
        tokenizer,
        device,
        dims: config.hidden_size,
        gpu: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static MODEL: OnceLock<LocalEmbedder> = OnceLock::new();

    fn model() -> &'static LocalEmbedder {
        MODEL.get_or_init(|| load().expect("load local bert model"))
    }

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        let (mut d, mut na, mut nb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(b.iter()) {
            d += x * y;
            na += x * x;
            nb += y * y;
        }
        d / (na.sqrt() * nb.sqrt())
    }

    const RELATED_A: &str = "The cat sat on the mat.";
    const RELATED_B: &str = "A cat rests on a rug.";
    const UNRELATED: &str = "Quantum chromodynamics fixes the gluon gauge.";

    fn have_model() -> bool {
        let ok = model_dir().join("config.json").exists();
        if !ok {
            eprintln!("skip: no local model at {}", model_dir().display());
        }
        ok
    }

    #[test]
    fn dims_are_768() {
        if !have_model() {
            return;
        }
        assert_eq!(model().dims(), 768);
        let v = model().embed(&[RELATED_A]).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), 768);
    }

    #[test]
    fn vectors_are_l2_normalized() {
        if !have_model() {
            return;
        }
        let vs = model().embed(&[RELATED_A, UNRELATED]).unwrap();
        for v in &vs {
            let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((n - 1.0).abs() < 1e-4, "norm was {n}");
        }
    }

    #[test]
    fn related_scores_higher_than_unrelated() {
        if !have_model() {
            return;
        }
        let vs = model().embed(&[RELATED_A, RELATED_B, UNRELATED]).unwrap();
        let related = cos(&vs[0], &vs[1]);
        let unrelated = cos(&vs[0], &vs[2]);
        assert!(
            related > unrelated,
            "related={related} unrelated={unrelated}"
        );
    }

    #[test]
    fn empty_batch_is_empty() {
        if !have_model() {
            return;
        }
        assert!(model().embed(&[]).unwrap().is_empty());
    }

    #[test]
    #[ignore = "needs CUDA GPU; run with -- --ignored (and --features cuda for device GPU)"]
    fn gpu_embed_smoke() {
        let device = Device::new_cuda(0).expect("CUDA device");
        assert!(device.is_cuda());
        let m = load_from_with_device(&model_dir(), device).expect("load on cuda");
        let v = m.embed(&[RELATED_A]).unwrap();
        assert_eq!(v[0].len(), 768);
    }
}

#[cfg(feature = "cuda")]
pub mod cuda;
#[cfg(all(test, feature = "cuda"))]
mod cuda_tests {
    use super::*;
    use std::sync::OnceLock;

    static CUDA_MODEL: OnceLock<Result<LocalEmbedder, String>> = OnceLock::new();

    fn cuda_model() -> Option<&'static LocalEmbedder> {
        let r = CUDA_MODEL.get_or_init(|| {
            if !model_dir().join("config.json").exists() {
                return Err("no model dir".into());
            }
            load().map_err(|e| format!("{e:#}"))
        });
        match r {
            Ok(m) if m.gpu.is_some() => Some(m),
            _ => None,
        }
    }

    fn load_cpu_only() -> anyhow::Result<LocalEmbedder> {
        load_candle(&model_dir(), Device::Cpu)
    }

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        let (mut d, mut na, mut nb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(b.iter()) {
            d += x * y;
            na += x * x;
            nb += y * y;
        }
        d / (na.sqrt() * nb.sqrt())
    }

    const A: &str = "The cat sat on the mat.";
    const B: &str = "A cat rests on a rug.";
    const C: &str = "Quantum chromodynamics fixes the gluon gauge.";

    #[test]
    fn cuda_load_and_dims() {
        let Some(m) = cuda_model() else {
            eprintln!("skip: cuda unavailable");
            return;
        };
        assert!(
            m.device_name().contains("Cuda"),
            "device: {}",
            m.device_name()
        );
        let v = m.embed(&[A]).unwrap();
        assert_eq!(v[0].len(), 768);
    }

    #[test]
    fn cuda_vectors_l2_normalized() {
        let Some(m) = cuda_model() else {
            eprintln!("skip");
            return;
        };
        for v in m.embed(&[A, C]).unwrap() {
            let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((n - 1.0).abs() < 1e-3, "norm {n}");
        }
    }

    #[test]
    fn cuda_matches_cpu_semantics() {
        let Some(m) = cuda_model() else {
            eprintln!("skip");
            return;
        };
        let vs = m.embed(&[A, B, C]).unwrap();
        let cpu = load_cpu_only().expect("cpu embedder");
        let refv = cpu.embed(&[A]).unwrap().pop().unwrap();
        let c = cos(&vs[0], &refv);
        assert!(c > 0.999, "gpu-vs-cpu cosine {c}");
        assert!(cos(&vs[0], &vs[1]) > cos(&vs[0], &vs[2]));
    }

    #[test]
    fn cuda_empty_batch() {
        let Some(m) = cuda_model() else {
            eprintln!("skip");
            return;
        };
        assert!(m.embed(&[]).unwrap().is_empty());
    }
}
