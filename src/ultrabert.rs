//! UltraBERT-Embed-150M forward pass on candle (CPU): token embeddings ->
//! 24 x [RMSNorm -> RoPE attention (sliding window ±128 on layers where
//! config.attention_windows[i] is set) -> residual -> RMSNorm -> SwiGLU MLP ->
//! residual] -> final RMSNorm. Pooling (CLS = position 0) and normalisation
//! happen in embed.rs.

use candle_core::{DType, Device, Result as CandleResult, Tensor};
use candle_nn::{ops, VarBuilder};

const HIDDEN: usize = 768;
const HEADS: usize = 6;
const HEAD_DIM: usize = 128;
const INTERMEDIATE: usize = 1024;
/// RoPE tables are precomputed up to this many positions; longer inputs fall
/// back to computing the table on the fly.
const ROPE_CACHE: usize = 1024;

struct Layer {
    input_norm: Tensor,
    post_norm: Tensor,
    /// q, k and v concatenated: (3 * HIDDEN, HIDDEN).
    qkv: Tensor,
    o: Tensor,
    /// gate and up concatenated: (2 * INTERMEDIATE, HIDDEN).
    gate_up: Tensor,
    down: Tensor,
    /// Index into `UltraBert::rope`.
    rope: usize,
    window: Option<usize>,
}

pub struct UltraBert {
    embedding: Tensor,
    layers: Vec<Layer>,
    norm: Tensor,
    /// (theta, cos, sin), each table (1, 1, ROPE_CACHE, HEAD_DIM).
    rope: Vec<(f64, Tensor, Tensor)>,
    device: Device,
}

fn linear(x: &Tensor, weight: &Tensor, out: usize) -> CandleResult<Tensor> {
    let dims = x.dims();
    let y = x
        .reshape((dims[0] * dims[1], dims[2]))?
        .matmul(&weight.t()?)?;
    y.reshape((dims[0], dims[1], out))
}

fn rope_tables(theta: f64, seq: usize, device: &Device) -> CandleResult<(Tensor, Tensor)> {
    let mut cos = Vec::with_capacity(seq * HEAD_DIM);
    let mut sin = Vec::with_capacity(seq * HEAD_DIM);
    for pos in 0..seq {
        // Both halves of the width carry the same angle; `rope` rotates each
        // half against the other.
        for _ in 0..2 {
            for i in 0..HEAD_DIM / 2 {
                let angle = pos as f64 / theta.powf((2 * i) as f64 / HEAD_DIM as f64);
                cos.push(angle.cos() as f32);
                sin.push(angle.sin() as f32);
            }
        }
    }
    Ok((
        Tensor::from_vec(cos, (1, 1, seq, HEAD_DIM), device)?,
        Tensor::from_vec(sin, (1, 1, seq, HEAD_DIM), device)?,
    ))
}

fn rope(x: &Tensor, cos: &Tensor, sin: &Tensor) -> CandleResult<Tensor> {
    let left = x.narrow(3, 0, HEAD_DIM / 2)?;
    let right = x.narrow(3, HEAD_DIM / 2, HEAD_DIM / 2)?;
    let rotated = Tensor::cat(&[&right.neg()?, &left], 3)?;
    x.broadcast_mul(cos)?.add(&rotated.broadcast_mul(sin)?)
}

fn attention_mask(
    batch: usize,
    seq: usize,
    lengths: &[usize],
    window: Option<usize>,
    device: &Device,
) -> CandleResult<Tensor> {
    let mut values = vec![0f32; batch * seq * seq];
    for b in 0..batch {
        for i in 0..seq {
            for j in 0..seq {
                let valid =
                    j < lengths[b] && i < lengths[b] && window.is_none_or(|w| i.abs_diff(j) <= w);
                values[(b * seq + i) * seq + j] = if valid { 0.0 } else { f32::MIN };
            }
        }
    }
    Tensor::from_vec(values, (batch, 1, seq, seq), device)
}

impl UltraBert {
    pub fn from_files(
        weights: &std::path::Path,
        config: &serde_json::Value,
    ) -> crate::Result<Self> {
        // candle asks gemm for `get_num_threads()` (the physical core count
        // unless RAYON_NUM_THREADS is set) while rayon's shared pool defaults
        // to logical cores; that mismatch costs ~50% on small batches, so
        // pin the pool to the size gemm will request.
        if std::env::var_os("RAYON_NUM_THREADS").is_none() {
            std::env::set_var(
                "RAYON_NUM_THREADS",
                candle_core::utils::get_num_threads().to_string(),
            );
        }
        let device = Device::Cpu;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device) }?;
        let rope_theta: Vec<f64> = serde_json::from_value(config["rope_theta"].clone())?;
        let windows: Vec<Option<usize>> =
            serde_json::from_value(config["attention_windows"].clone())?;
        // A wrong config must be an error, not an index panic that kills the MCP server.
        if rope_theta.len() < 24 || windows.len() < 24 {
            return Err(crate::err!(
                "model config has {} rope_theta and {} attention_windows entries; UltraBERT-Embed-150M needs 24 each",
                rope_theta.len(),
                windows.len()
            ));
        }

        let mut rope: Vec<(f64, Tensor, Tensor)> = Vec::new();
        let mut rope_index = |theta: f64| -> crate::Result<usize> {
            if let Some(index) = rope.iter().position(|(t, _, _)| *t == theta) {
                return Ok(index);
            }
            let (cos, sin) = rope_tables(theta, ROPE_CACHE, &device)?;
            rope.push((theta, cos, sin));
            Ok(rope.len() - 1)
        };

        let mut layers = Vec::with_capacity(24);
        for i in 0..24 {
            let p = vb.pp(format!("layers.{i}"));
            let attn = p.pp("self_attn");
            let mlp = p.pp("mlp");
            let qkv = Tensor::cat(
                &[
                    attn.get((HIDDEN, HIDDEN), "q_proj.weight")?,
                    attn.get((HIDDEN, HIDDEN), "k_proj.weight")?,
                    attn.get((HIDDEN, HIDDEN), "v_proj.weight")?,
                ],
                0,
            )?;
            let gate_up = Tensor::cat(
                &[
                    mlp.get((INTERMEDIATE, HIDDEN), "gate_proj.weight")?,
                    mlp.get((INTERMEDIATE, HIDDEN), "up_proj.weight")?,
                ],
                0,
            )?;
            layers.push(Layer {
                input_norm: p.pp("input_layernorm").get((HIDDEN,), "weight")?,
                post_norm: p.pp("post_attention_layernorm").get((HIDDEN,), "weight")?,
                qkv,
                o: attn.get((HIDDEN, HIDDEN), "o_proj.weight")?,
                gate_up,
                down: mlp.get((HIDDEN, INTERMEDIATE), "down_proj.weight")?,
                rope: rope_index(rope_theta[i])?,
                window: windows[i],
            });
        }
        Ok(Self {
            embedding: vb.get((49152, HIDDEN), "embed_tokens.weight")?,
            layers,
            norm: vb.get((HIDDEN,), "norm.weight")?,
            rope,
            device,
        })
    }

    pub fn forward(&self, ids: &Tensor, lengths: &[usize]) -> CandleResult<Tensor> {
        let (batch, seq) = (ids.dims()[0], ids.dims()[1]);
        let mut x = self
            .embedding
            .index_select(&ids.flatten_all()?, 0)?
            .reshape((batch, seq, HIDDEN))?;
        // Masks only matter for padding or when a window is narrower than the
        // sequence; build each distinct one at most once per forward.
        let padded = lengths.iter().any(|&len| len < seq);
        let mut masks: Vec<(Option<usize>, Tensor)> = Vec::new();
        // RoPE tables beyond the cached range, one per distinct theta.
        let long_rope = if seq > ROPE_CACHE {
            Some(
                self.rope
                    .iter()
                    .map(|(theta, _, _)| rope_tables(*theta, seq, &self.device))
                    .collect::<CandleResult<Vec<_>>>()?,
            )
        } else {
            None
        };
        for layer in &self.layers {
            let mask = if padded || layer.window.is_some_and(|w| seq > w) {
                let index = match masks.iter().position(|(w, _)| *w == layer.window) {
                    Some(index) => index,
                    None => {
                        let mask = attention_mask(batch, seq, lengths, layer.window, &self.device)?;
                        masks.push((layer.window, mask));
                        masks.len() - 1
                    }
                };
                Some(&masks[index].1)
            } else {
                None
            };

            let (cos, sin) = match &long_rope {
                Some(tables) => {
                    let (cos, sin) = &tables[layer.rope];
                    (cos.clone(), sin.clone())
                }
                None => {
                    let (_, cos, sin) = &self.rope[layer.rope];
                    (cos.narrow(2, 0, seq)?, sin.narrow(2, 0, seq)?)
                }
            };

            let normalized = ops::rms_norm(&x, &layer.input_norm, 1e-6)?;
            let qkv = linear(&normalized, &layer.qkv, 3 * HIDDEN)?;
            let q = qkv
                .narrow(2, 0, HIDDEN)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let k = qkv
                .narrow(2, HIDDEN, HIDDEN)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let v = qkv
                .narrow(2, 2 * HIDDEN, HIDDEN)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let q = rope(&q, &cos, &sin)?;
            let k = rope(&k, &cos, &sin)?;
            let scores = q
                .matmul(&k.transpose(2, 3)?)?
                .affine(1.0 / (HEAD_DIM as f64).sqrt(), 0.0)?;
            let scores = match mask {
                Some(mask) => scores.broadcast_add(mask)?,
                None => scores,
            };
            let weights = ops::softmax_last_dim(&scores)?;
            let attended = weights
                .matmul(&v)?
                .transpose(1, 2)?
                .contiguous()?
                .reshape((batch, seq, HEADS * HEAD_DIM))?;
            x = x.add(&linear(&attended, &layer.o, HIDDEN)?)?;

            let normalized = ops::rms_norm(&x, &layer.post_norm, 1e-6)?;
            let gate_up = linear(&normalized, &layer.gate_up, 2 * INTERMEDIATE)?;
            let gated = ops::silu(&gate_up.narrow(2, 0, INTERMEDIATE)?)?.mul(&gate_up.narrow(
                2,
                INTERMEDIATE,
                INTERMEDIATE,
            )?)?;
            x = x.add(&linear(&gated, &layer.down, HIDDEN)?)?;
        }
        ops::rms_norm(&x, &self.norm, 1e-6)
    }
}
