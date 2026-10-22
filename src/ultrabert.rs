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

struct Layer {
    input_norm: Tensor,
    post_norm: Tensor,
    q: Tensor,
    k: Tensor,
    v: Tensor,
    o: Tensor,
    gate: Tensor,
    up: Tensor,
    down: Tensor,
    theta: f64,
    window: Option<usize>,
}

pub struct UltraBert {
    embedding: Tensor,
    layers: Vec<Layer>,
    norm: Tensor,
    device: Device,
}

fn linear(x: &Tensor, weight: &Tensor, out: usize) -> CandleResult<Tensor> {
    let dims = x.dims();
    let y = x
        .reshape((dims[0] * dims[1], dims[2]))?
        .matmul(&weight.t()?)?;
    y.reshape((dims[0], dims[1], out))
}

fn rms(x: &Tensor, weight: &Tensor) -> CandleResult<Tensor> {
    let variance = x.to_dtype(DType::F32)?.sqr()?.mean_keepdim(2)?;
    let scale = variance.affine(1.0, 1e-6)?.sqrt()?.recip()?;
    x.broadcast_mul(&scale)?.broadcast_mul(weight)
}

fn rope(x: &Tensor, theta: f64, seq: usize) -> CandleResult<Tensor> {
    let mut values = Vec::with_capacity(seq * HEAD_DIM);
    for pos in 0..seq {
        for i in 0..HEAD_DIM / 2 {
            let angle = pos as f64 / theta.powf((2 * i) as f64 / HEAD_DIM as f64);
            values.push(angle.cos() as f32);
        }
        for i in 0..HEAD_DIM / 2 {
            let angle = pos as f64 / theta.powf((2 * i) as f64 / HEAD_DIM as f64);
            values.push(angle.cos() as f32);
        }
    }
    let cos = Tensor::from_vec(values, (1, 1, seq, HEAD_DIM), x.device())?;
    let mut values = Vec::with_capacity(seq * HEAD_DIM);
    for pos in 0..seq {
        for i in 0..HEAD_DIM / 2 {
            let angle = pos as f64 / theta.powf((2 * i) as f64 / HEAD_DIM as f64);
            values.push(angle.sin() as f32);
        }
        for i in 0..HEAD_DIM / 2 {
            let angle = pos as f64 / theta.powf((2 * i) as f64 / HEAD_DIM as f64);
            values.push(angle.sin() as f32);
        }
    }
    let sin = Tensor::from_vec(values, (1, 1, seq, HEAD_DIM), x.device())?;
    let left = x.narrow(3, 0, HEAD_DIM / 2)?;
    let right = x.narrow(3, HEAD_DIM / 2, HEAD_DIM / 2)?;
    let rotated = Tensor::cat(&[&right.neg()?, &left], 3)?;
    x.broadcast_mul(&cos)?.add(&rotated.broadcast_mul(&sin)?)
}

impl UltraBert {
    pub fn from_files(
        weights: &std::path::Path,
        config: &serde_json::Value,
    ) -> crate::Result<Self> {
        let device = Device::Cpu;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device) }?;
        let rope_theta: Vec<f64> = serde_json::from_value(config["rope_theta"].clone())?;
        let windows: Vec<Option<usize>> =
            serde_json::from_value(config["attention_windows"].clone())?;
        let mut layers = Vec::with_capacity(24);
        for i in 0..24 {
            let p = vb.pp(format!("layers.{i}"));
            layers.push(Layer {
                input_norm: p.pp("input_layernorm").get((HIDDEN,), "weight")?,
                post_norm: p.pp("post_attention_layernorm").get((HIDDEN,), "weight")?,
                q: p.pp("self_attn")
                    .get((HEADS * HEAD_DIM, HIDDEN), "q_proj.weight")?,
                k: p.pp("self_attn")
                    .get((HEADS * HEAD_DIM, HIDDEN), "k_proj.weight")?,
                v: p.pp("self_attn")
                    .get((HEADS * HEAD_DIM, HIDDEN), "v_proj.weight")?,
                o: p.pp("self_attn")
                    .get((HIDDEN, HEADS * HEAD_DIM), "o_proj.weight")?,
                gate: p
                    .pp("mlp")
                    .get((INTERMEDIATE, HIDDEN), "gate_proj.weight")?,
                up: p.pp("mlp").get((INTERMEDIATE, HIDDEN), "up_proj.weight")?,
                down: p
                    .pp("mlp")
                    .get((HIDDEN, INTERMEDIATE), "down_proj.weight")?,
                theta: rope_theta[i],
                window: windows[i],
            });
        }
        Ok(Self {
            embedding: vb.get((49152, HIDDEN), "embed_tokens.weight")?,
            layers,
            norm: vb.get((HIDDEN,), "norm.weight")?,
            device,
        })
    }

    pub fn forward(&self, ids: &Tensor, lengths: &[usize]) -> CandleResult<Tensor> {
        let mut x = self
            .embedding
            .index_select(&ids.flatten_all()?, 0)?
            .reshape((ids.dims()[0], ids.dims()[1], HIDDEN))?;
        let batch = lengths.len();
        let seq = ids.dims()[1];
        for layer in &self.layers {
            let normalized = rms(&x, &layer.input_norm)?;
            let q = linear(&normalized, &layer.q, HEADS * HEAD_DIM)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let k = linear(&normalized, &layer.k, HEADS * HEAD_DIM)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let v = linear(&normalized, &layer.v, HEADS * HEAD_DIM)?
                .reshape((batch, seq, HEADS, HEAD_DIM))?
                .transpose(1, 2)?
                .contiguous()?;
            let q = rope(&q, layer.theta, seq)?;
            let k = rope(&k, layer.theta, seq)?;
            let scores = q
                .matmul(&k.transpose(2, 3)?)?
                .affine(1.0 / (HEAD_DIM as f64).sqrt(), 0.0)?;
            let mut layer_mask = vec![0f32; batch * seq * seq];
            for b in 0..batch {
                for i in 0..seq {
                    for j in 0..seq {
                        let valid = j < lengths[b]
                            && i < lengths[b]
                            && layer.window.is_none_or(|w| i.abs_diff(j) <= w);
                        layer_mask[(b * seq + i) * seq + j] = if valid { 0.0 } else { f32::MIN };
                    }
                }
            }
            let mask = Tensor::from_vec(layer_mask, (batch, 1, seq, seq), &self.device)?;
            let weights = ops::softmax_last_dim(&scores.broadcast_add(&mask)?)?;
            let attended = weights
                .matmul(&v)?
                .transpose(1, 2)?
                .contiguous()?
                .reshape((batch, seq, HEADS * HEAD_DIM))?;
            x = x.add(&linear(&attended, &layer.o, HIDDEN)?)?;
            let normalized = rms(&x, &layer.post_norm)?;
            let gated = ops::silu(&linear(&normalized, &layer.gate, INTERMEDIATE)?)?
                .mul(&linear(&normalized, &layer.up, INTERMEDIATE)?)?;
            x = x.add(&linear(&gated, &layer.down, HIDDEN)?)?;
        }
        rms(&x, &self.norm)
    }
}
