//! Minimal f32 GPU BERT embedder via cudarc (nvrtc PTX + cublas sgemm).
//!
//! DEPRECATED backend: exists only because candle-kernels cannot compile against
//! CUDA 13.x on sm_75 (`__hmax_nan`/`__hmin_nan` collision, docs/cuda-unblock.md).
//! Remove this module and the `cuda` feature once candle natively supports sm_75
//! on CUDA 13.x (upstream huggingface/candle#3909 in a released candle version).
//!
//! Design: candle-cuda is blocked by candle-kernels vs CUDA 13.3/sm_75 and patching
//! it is env surgery; cudarc 0.19 is already in the lockfile and never runs nvcc on
//! candle kernels — it compiles small PTX strings via libnvrtc at runtime. BERT
//! forward needs only cublas sgemm plus a handful of elementwise/layernorm/softmax
//! kernels, so this ~500-line backend is cheaper than porting candle's kernel suite.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cudarc::cublas::safe::{CudaBlas, Gemm, StridedBatchedConfig};
use cudarc::cublas::sys::cublasOperation_t;
use cudarc::driver::{CudaContext, CudaSlice, CudaStream, LaunchConfig, PushKernelArg};
use cudarc::nvrtc::{compile_ptx_with_opts, CompileOptions};

use crate::{Embedder, MAX_LEN};

const PTX_SRC: &str = r#"
extern "C" __global__ void add_bias(float* x, const float* b, int total, int width) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < total) x[i] += b[i % width];
}

extern "C" __global__ void add_resid(float* y, const float* x, const float* r, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) y[i] = x[i] + r[i];
}

extern "C" __global__ void gelu(float* x, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        float v = x[i];
        float c = 0.044715f * v * v * v;
        x[i] = 0.5f * v * (1.0f + tanhf(0.7978845608028654f * (v + c)));
    }
}

// one block per row, blockDim.x == width (768)
extern "C" __global__ void layernorm(const float* x, const float* gamma, const float* beta,
                                     float* y, int rows, int width, float eps) {
    int row = blockIdx.x;
    int t = threadIdx.x;
    if (row >= rows || t >= width) return;
    float v = x[row * width + t];
    __shared__ float s_sum[2];
    __shared__ float mean, inv_std;
    if (t == 0) { s_sum[0] = 0.0f; s_sum[1] = 0.0f; }
    __syncthreads();
    atomicAdd(&s_sum[0], v);
    atomicAdd(&s_sum[1], v * v);
    __syncthreads();
    if (t == 0) {
        mean = s_sum[0] / width;
        float ex2 = s_sum[1] / width;
        float var = fmaxf(ex2 - mean * mean, 0.0f);
        inv_std = rsqrtf(var + eps);
    }
    __syncthreads();
    y[row * width + t] = (v - mean) * inv_std * gamma[t] + beta[t];
}

// rowwise masked softmax over [rows, s]; grid = rows, dynamic smem = s floats.
// row = ((b * heads + head) * w + qrow); mask is [n, s] indexed by (b, qrow).
extern "C" __global__ void softmax_masked(const float* x, const float* mask, float* y,
                                          int rows, int s, int qrows) {
    int row = blockIdx.x;
    if (row >= rows) return;
    int b = row / qrows;
    int qrow = row % qrows;
    const float* mrow = mask + b * s;
    extern __shared__ float red[];
    float local_max = -INFINITY;
    for (int i = threadIdx.x; i < s; i += blockDim.x) {
        float m = mrow[qrow * s + i];
        float v = x[row * s + i] + (m > 0.0f ? 0.0f : -1e9f);
        red[i] = v;
        if (v > local_max) local_max = v;
    }
    __shared__ float row_max, row_sum;
    if (threadIdx.x == 0) { row_max = -INFINITY; row_sum = 0.0f; }
    __syncthreads();
    atomicMax((int*)&row_max, __float_as_int(local_max));
    __syncthreads();
    float local_sum = 0.0f;
    for (int i = threadIdx.x; i < s; i += blockDim.x) {
        float e = expf(red[i] - row_max);
        red[i] = e;
        local_sum += e;
    }
    __syncthreads();
    atomicAdd(&row_sum, local_sum);
    __syncthreads();
    for (int i = threadIdx.x; i < s; i += blockDim.x) {
        y[row * s + i] = red[i] / row_sum;
    }
}
"#;

// ---------------- safetensors (f32 only) ----------------

#[derive(serde::Deserialize)]
struct StHeader {
    #[serde(rename = "__metadata__", default)]
    _meta: Option<serde_json::Value>,
    #[serde(flatten)]
    entries: std::collections::BTreeMap<String, StEntry>,
}

#[derive(serde::Deserialize)]
struct StEntry {
    dtype: String,
    data_offsets: [usize; 2],
}

struct Tensors {
    buf: Vec<u8>,
    map: std::collections::BTreeMap<String, (usize, usize)>,
}

fn load_safetensors(path: &Path) -> Result<Tensors> {
    let buf = std::fs::read(path).context("read model.safetensors")?;
    if buf.len() < 8 {
        return Err(anyhow!("safetensors too small"));
    }
    let n = u64::from_le_bytes(buf[..8].try_into().unwrap()) as usize;
    if 8 + n > buf.len() {
        return Err(anyhow!("safetensors header out of bounds"));
    }
    let hdr: StHeader =
        serde_json::from_slice(&buf[8..8 + n]).context("parse safetensors header")?;
    let base = 8 + n;
    let mut map = std::collections::BTreeMap::new();
    for (k, v) in hdr.entries {
        if v.dtype != "F32" {
            return Err(anyhow!("tensor {k} dtype {} != F32", v.dtype));
        }
        let [a, b] = v.data_offsets;
        if base + b > buf.len() {
            return Err(anyhow!("tensor {k} out of bounds"));
        }
        map.insert(k, (base + a, b - a));
    }
    Ok(Tensors { buf, map })
}

impl Tensors {
    fn get(&self, name: &str) -> Result<Vec<f32>> {
        let (off, len) = self
            .map
            .get(name)
            .ok_or_else(|| anyhow!("missing tensor {name}"))?;
        if len % 4 != 0 || *off % 4 != 0 {
            return Err(anyhow!("tensor {name} not f32-aligned"));
        }
        let bytes = &self.buf[*off..*off + *len];
        // Safe: f32 array from an aligned, bounds-checked byte slice.
        let floats = unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, len / 4) };
        Ok(floats.to_vec())
    }
}

// ---------------- weights ----------------

struct Mat {
    w: Vec<f32>, // row-major [out, in]
    b: Vec<f32>,
    out: usize,
}

fn mat(t: &Tensors, prefix: &str) -> Result<Mat> {
    let w = t.get(&format!("{prefix}.weight"))?;
    let b = t.get(&format!("{prefix}.bias"))?;
    let out = b.len();
    if w.is_empty() || out == 0 || w.len() % out != 0 {
        return Err(anyhow!("bad mat {prefix}: w={} b={}", w.len(), out));
    }
    let inp = w.len() / out;
    assert_eq!(w.len(), out * inp, "mat dims inconsistent");
    let _ = inp; // validated above
    Ok(Mat { w, b, out })
}

struct Layer {
    q: Mat,
    k: Mat,
    v: Mat,
    attn_out: Mat,
    ln1w: Vec<f32>,
    ln1b: Vec<f32>,
    ff1: Mat,
    ff2: Mat,
    ln2w: Vec<f32>,
    ln2b: Vec<f32>,
}

struct Weights {
    wte: Vec<f32>,
    wpe: Vec<f32>,
    tok_type: Vec<f32>,
    emb_ln_w: Vec<f32>,
    emb_ln_b: Vec<f32>,
    layers: Vec<Layer>,
    v: usize,
    p: usize,
    h: usize,
    heads: usize,
}

fn load_weights(dir: &Path) -> Result<Weights> {
    let t = load_safetensors(&dir.join("model.safetensors"))?;
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("config.json")).context("read config.json")?,
    )?;
    let g = |k: &str| {
        config[k]
            .as_u64()
            .map(|x| x as usize)
            .ok_or_else(|| anyhow!("config missing {k}"))
    };
    let (h, heads) = (g("hidden_size")?, g("num_attention_heads")?);
    if h % heads != 0 {
        return Err(anyhow!("hidden_size {h} not divisible by {heads} heads"));
    }
    let layers = (0..g("num_hidden_layers")?)
        .map(|i| {
            let pre = format!("encoder.layer.{i}.");
            Ok(Layer {
                q: mat(&t, &format!("{pre}attention.self.query"))?,
                k: mat(&t, &format!("{pre}attention.self.key"))?,
                v: mat(&t, &format!("{pre}attention.self.value"))?,
                attn_out: mat(&t, &format!("{pre}attention.output.dense"))?,
                ln1w: t.get(&format!("{pre}attention.output.LayerNorm.weight"))?,
                ln1b: t.get(&format!("{pre}attention.output.LayerNorm.bias"))?,
                ff1: mat(&t, &format!("{pre}intermediate.dense"))?,
                ff2: mat(&t, &format!("{pre}output.dense"))?,
                ln2w: t.get(&format!("{pre}output.LayerNorm.weight"))?,
                ln2b: t.get(&format!("{pre}output.LayerNorm.bias"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Weights {
        wte: t.get("embeddings.word_embeddings.weight")?,
        wpe: t.get("embeddings.position_embeddings.weight")?,
        tok_type: t.get("embeddings.token_type_embeddings.weight")?,
        emb_ln_w: t.get("embeddings.LayerNorm.weight")?,
        emb_ln_b: t.get("embeddings.LayerNorm.bias")?,
        layers,
        v: g("vocab_size")?,
        p: g("max_position_embeddings")?,
        h,
        heads,
    })
}

// ---------------- GPU runtime ----------------

pub struct CudaEmbedder {
    weights: Weights,
    tokenizer: tokenizers::Tokenizer,
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    blas: CudaBlas,
    fns: Kernels,
    dev: DevTensors,
    dims: usize,
}

struct Kernels {
    add_bias: cudarc::driver::CudaFunction,
    add_resid: cudarc::driver::CudaFunction,
    gelu: cudarc::driver::CudaFunction,
    layernorm: cudarc::driver::CudaFunction,
    softmax: cudarc::driver::CudaFunction,
}

struct DevTensors {
    emb_ln_w: CudaSlice<f32>,
    emb_ln_b: CudaSlice<f32>,
    layers: Vec<DevLayer>,
}

struct DevLayer {
    qw: CudaSlice<f32>,
    qb: CudaSlice<f32>,
    kw: CudaSlice<f32>,
    kb: CudaSlice<f32>,
    vw: CudaSlice<f32>,
    vb: CudaSlice<f32>,
    aw: CudaSlice<f32>,
    ab: CudaSlice<f32>,
    ln1w: CudaSlice<f32>,
    ln1b: CudaSlice<f32>,
    f1w: CudaSlice<f32>,
    f1b: CudaSlice<f32>,
    f2w: CudaSlice<f32>,
    f2b: CudaSlice<f32>,
    ln2w: CudaSlice<f32>,
    ln2b: CudaSlice<f32>,
}

fn upload(stream: &Arc<CudaStream>, v: &[f32]) -> Result<CudaSlice<f32>> {
    Ok(stream.clone_htod(v)?)
}

impl CudaEmbedder {
    pub fn load(dir: &Path) -> Result<CudaEmbedder> {
        let ctx = CudaContext::new(0)?;
        let stream = ctx.default_stream();
        let opts = CompileOptions {
            arch: Some("compute_75"),
            ftz: Some(true),
            ..Default::default()
        };
        let ptx = compile_ptx_with_opts(PTX_SRC, opts).map_err(|e| anyhow!("nvrtc: {e}"))?;
        let module = ctx.load_module(ptx)?;
        let fns = Kernels {
            add_bias: module.load_function("add_bias")?,
            add_resid: module.load_function("add_resid")?,
            gelu: module.load_function("gelu")?,
            layernorm: module.load_function("layernorm")?,
            softmax: module.load_function("softmax_masked")?,
        };
        let blas = CudaBlas::new(stream.clone())?;
        let weights = load_weights(dir)?;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("load tokenizer.json: {e}"))?;
        let dev = DevTensors {
            emb_ln_w: upload(&stream, &weights.emb_ln_w)?,
            emb_ln_b: upload(&stream, &weights.emb_ln_b)?,
            layers: weights
                .layers
                .iter()
                .map(|l| {
                    Ok(DevLayer {
                        qw: upload(&stream, &l.q.w)?,
                        qb: upload(&stream, &l.q.b)?,
                        kw: upload(&stream, &l.k.w)?,
                        kb: upload(&stream, &l.k.b)?,
                        vw: upload(&stream, &l.v.w)?,
                        vb: upload(&stream, &l.v.b)?,
                        aw: upload(&stream, &l.attn_out.w)?,
                        ab: upload(&stream, &l.attn_out.b)?,
                        ln1w: upload(&stream, &l.ln1w)?,
                        ln1b: upload(&stream, &l.ln1b)?,
                        f1w: upload(&stream, &l.ff1.w)?,
                        f1b: upload(&stream, &l.ff1.b)?,
                        f2w: upload(&stream, &l.ff2.w)?,
                        f2b: upload(&stream, &l.ff2.b)?,
                        ln2w: upload(&stream, &l.ln2w)?,
                        ln2b: upload(&stream, &l.ln2b)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        };
        Ok(CudaEmbedder {
            dims: weights.h,
            weights,
            tokenizer,
            ctx,
            stream,
            blas,
            fns,
            dev,
        })
    }

    pub fn device_name(&self) -> String {
        format!("Cuda({})", self.ctx.name().unwrap_or_default())
    }

    pub(crate) fn tokenizer_clone(&self) -> tokenizers::Tokenizer {
        self.tokenizer.clone()
    }

    /// Full BERT forward on GPU; returns final hidden states [n * width * 768] host-side.
    fn forward(&self, n: usize, width: usize, ids: &[u32], mask: &[u32]) -> Result<Vec<f32>> {
        let h = self.weights.h;
        let heads = self.weights.heads;
        let hk = h / heads;
        let scale = 1.0f32 / (hk as f32).sqrt();
        let total = n * width * h;
        let eps = 1e-12f32;

        // Embeddings: gather on host (cheap), single H2D copy.
        let mut emb = vec![0.0f32; total];
        for r in 0..n {
            for c in 0..width {
                let id = ids[r * width + c] as usize;
                if id >= self.weights.v || c >= self.weights.p {
                    return Err(anyhow!("token id {id} or position {c} out of range"));
                }
                let dst = (r * width + c) * h;
                emb[dst..dst + h].copy_from_slice(&self.weights.wte[id * h..id * h + h]);
                for (i, xx) in emb[dst..dst + h].iter_mut().enumerate() {
                    *xx += self.weights.wpe[c * h + i] + self.weights.tok_type[i];
                }
            }
        }
        let mut x = self.stream.clone_htod(&emb)?;
        self.layernorm(
            &mut x,
            &self.dev.emb_ln_w,
            &self.dev.emb_ln_b,
            n * width,
            h,
            eps,
        )?;

        let ff_dim = self.weights.layers[0].ff1.out;
        let mut proj = self.stream.alloc_zeros::<f32>(total * 3)?; // qkv concat [n*w, 3h]
        let mut attn = self.stream.alloc_zeros::<f32>(total)?; // attention context [n*w, h]
        let mut scores = self.stream.alloc_zeros::<f32>(n * heads * width * width)?;
        let mut resid = self.stream.alloc_zeros::<f32>(total)?;
        let mut ff = self.stream.alloc_zeros::<f32>(n * width * ff_dim)?;
        let mask_dev = self.stream.clone_htod(mask)?;

        for (li, dl) in self.dev.layers.iter().enumerate() {
            let f1out = self.weights.layers[li].ff1.out;
            // resid <- x (device-to-device)
            self.stream.memcpy_dtod(&x, &mut resid)?;
            // pre-norm: x <- LN(x)
            self.layernorm(&mut x, &dl.ln1w, &dl.ln1b, n * width, h, eps)?;

            // qkv projection: x [n*w, h] @ [h, out]^T + bias, q/k/v side by side in proj
            self.gemm_bias(&x, n * width, h, &dl.qw, &dl.qb, &mut proj, 0)?;
            self.gemm_bias(&x, n * width, h, &dl.kw, &dl.kb, &mut proj, h)?;
            self.gemm_bias(&x, n * width, h, &dl.vw, &dl.vb, &mut proj, 2 * h)?;

            // scores[b,head,j,i] (j = query row) = scale * q . k ; softmax over i; ctx = P @ V
            self.attention(
                n,
                width,
                heads,
                hk,
                scale,
                &proj,
                &mut scores,
                &mut attn,
                &mask_dev,
            )?;

            // attn output projection + bias, then residual
            self.gemm_bias(&attn, n * width, h, &dl.aw, &dl.ab, &mut x, 0)?;
            self.add_resid(&mut x, &resid, total)?;

            // FFN
            self.stream.memcpy_dtod(&x, &mut resid)?;
            self.layernorm(&mut x, &dl.ln2w, &dl.ln2b, n * width, h, eps)?;
            self.gemm_bias(&x, n * width, h, &dl.f1w, &dl.f1b, &mut ff, 0)?;
            self.gelu(&mut ff, n * width * f1out)?;
            self.gemm_bias(&ff, n * width, f1out, &dl.f2w, &dl.f2b, &mut x, 0)?;
            self.add_resid(&mut x, &resid, total)?;
        }
        self.stream.synchronize()?;
        Ok(self.stream.clone_dtoh(&x)?)
    }

    /// scores[b, head, j, i]: for each (b, head) pair, a [w, w] tile = q_tile @ k_tile^T * scale.
    /// proj row (b,i): [q(h) | k(h) | v(h)]; q tile of (b,head): proj[b*w*3h + i*3h + head*hk ..+hk]
    /// batch = b*heads + head; stride_a/b between heads = hk (uniform!), batch_size = n*heads.
    fn attention(
        &self,
        n: usize,
        width: usize,
        heads: usize,
        hk: usize,
        scale: f32,
        proj: &CudaSlice<f32>,
        scores: &mut CudaSlice<f32>,
        out: &mut CudaSlice<f32>,
        mask_dev: &CudaSlice<u32>,
    ) -> Result<()> {
        let h = self.weights.h;
        let h3 = 3 * h;
        let w = width;
        // One strided-batched gemm per document: batch = heads, stride hk (uniform within
        // a document); documents looped over (n small). scores[b, head] tiles [w, w].
        for b in 0..n {
            let base = b * w * h3;
            // k tiles: base + h; q tiles: base; v tiles: base + 2h
            let a = proj.slice(base + h..base + h + w * h3); // k [w, h], head d at offset head*hk
            let bt = proj.slice(base..base + w * h3); // q
            let mut c = scores.slice_mut(b * heads * w * w..(b + 1) * heads * w * w);
            let cfgb = StridedBatchedConfig::<f32> {
                gemm: cudarc::cublas::safe::GemmConfig {
                    transa: cublasOperation_t::CUBLAS_OP_N,
                    transb: cublasOperation_t::CUBLAS_OP_T,
                    m: w as i32,
                    n: w as i32,
                    k: hk as i32,
                    alpha: scale,
                    lda: h3 as i32,
                    ldb: h3 as i32,
                    beta: 0.0,
                    ldc: w as i32,
                },
                batch_size: heads as i32,
                stride_a: hk as i64,
                stride_b: hk as i64,
                stride_c: (w * w) as i64,
            };
            unsafe { self.blas.gemm_strided_batched(cfgb, &a, &bt, &mut c)? };
        }
        // softmax over last dim of each [w, w] tile (mask applies per document row)
        {
            let rows = (n * heads * w) as i32;
            let w_i = w as i32;
            let qrows = w as i32;
            let mut b = self.stream.launch_builder(&self.fns.softmax);
            b.arg(&*scores);
            b.arg(&*mask_dev);
            b.arg(&mut *out);
            b.arg(&rows);
            b.arg(&w_i);
            b.arg(&qrows);
            let cfgl = LaunchConfig {
                grid_dim: (rows as u32, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: (w * 4) as u32,
            };
            unsafe { b.launch(cfgl)? };
        }
        // ctx = P @ V per (b, head): P [w, w] row-major (query j, key i), V tiles [w, hk].
        // C[j, d] = sum_i P[j, i] V[i, d]; col-major: C^T[d, j] = sum_i V^T[d, i] P^T[i, j]
        // opA = V tiles [w, hk] transa=T -> [hk, w]; opB = P [w, w] transb=N? B[d, j] with ldb...
        // cublas: C[m, n] col-major = opA[m, k] opB[k, n]. Set C^T[d, j]: m = hk, n = w, k = w.
        // opA[d, i] = V[i, d] -> A = V tiles [w, hk] with transa = T, lda = h3
        // opB[i, j] = P[j, i] -> B = P tiles row-major [w(w_query), w(w_key)] i.e. B = scores tile,
        //   interpreted col-major as [w_key, w_query]: B[i, j] = scores[j, i] -> transb = N, ldb = w
        // C = ctx^T col-major [hk, w] -> ldc = hk, out tile d-major: ctx[b,i,head,d] at
        //   b*w*h3 + i*3h + 2h + head*hk + d -> C col-major element (d, j) at j*ldc + d =
        //   matches row j=i of ctx when ldc = h3 and base = b*w*h3 + 2h + head*hk. head stride hk.
        for b in 0..n {
            let base = b * w * h3;
            let a = proj.slice(base + 2 * h..base + 2 * h + w * h3); // v tiles
            let bt = scores.slice(b * heads * w * w..(b + 1) * heads * w * w);
            let mut c = out.slice_mut(base + 2 * h..base + 2 * h + w * h3);
            let cfgb = StridedBatchedConfig::<f32> {
                gemm: cudarc::cublas::safe::GemmConfig {
                    transa: cublasOperation_t::CUBLAS_OP_T,
                    transb: cublasOperation_t::CUBLAS_OP_N,
                    m: hk as i32,
                    n: w as i32,
                    k: w as i32,
                    alpha: 1.0,
                    lda: h3 as i32,
                    ldb: w as i32,
                    beta: 0.0,
                    ldc: h3 as i32,
                },
                batch_size: heads as i32,
                stride_a: hk as i64,
                stride_b: (w * w) as i64,
                stride_c: hk as i64,
            };
            unsafe { self.blas.gemm_strided_batched(cfgb, &a, &bt, &mut c)? };
        }
        Ok(())
    }

    fn add_resid(&self, x: &mut CudaSlice<f32>, r: &CudaSlice<f32>, n: usize) -> Result<()> {
        let nn = n as i32;
        let mut b = self.stream.launch_builder(&self.fns.add_resid);
        b.arg(x);
        b.arg(r);
        b.arg(&nn);
        unsafe { b.launch(launch_cfg(n as u32, 256))? };
        Ok(())
    }

    fn gelu(&self, x: &mut CudaSlice<f32>, n: usize) -> Result<()> {
        let nn = n as i32;
        let mut b = self.stream.launch_builder(&self.fns.gelu);
        b.arg(x);
        b.arg(&nn);
        unsafe { b.launch(launch_cfg(n as u32, 256))? };
        Ok(())
    }

    fn layernorm(
        &self,
        x: &mut CudaSlice<f32>,
        w: &CudaSlice<f32>,
        bb: &CudaSlice<f32>,
        rows: usize,
        width: usize,
        eps: f32,
    ) -> Result<()> {
        let rows_i = rows as i32;
        let width_i = width as i32;
        let mut builder = self.stream.launch_builder(&self.fns.layernorm);
        builder.arg(x);
        builder.arg(w);
        builder.arg(bb);
        builder.arg(&rows_i);
        builder.arg(&width_i);
        builder.arg(&eps);
        let cfg = LaunchConfig {
            grid_dim: (rows as u32, 1, 1),
            block_dim: (width as u32, 1, 1),
            shared_mem_bytes: 0,
        };
        unsafe { builder.launch(cfg)? };
        Ok(())
    }

    /// out[.., off..off+od] = a[.., id] @ W[od, id]^T + b ; W row-major [od, id].
    /// Row-major C = A @ W^T is col-major C^T = W @ A^T: m = od, n = rows, k = id;
    /// opA = W [od, id] transa=N, lda=id; opB = A [rows, id] transb=T, ldb=id;
    /// C col-major [od, rows], ldc=od -> element (d, r) at r*od + d = row-major C[r, d].
    fn gemm_bias(
        &self,
        a: &CudaSlice<f32>,
        rows: usize,
        in_dim: usize,
        w: &CudaSlice<f32>,
        bias: &CudaSlice<f32>,
        out: &mut CudaSlice<f32>,
        out_col_offset: usize,
    ) -> Result<()> {
        let od = w.len() / in_dim;
        let n_i = rows as i32;
        let _ = n_i;
        // single non-batched gemm
        let cfg = cudarc::cublas::safe::GemmConfig::<f32> {
            transa: cublasOperation_t::CUBLAS_OP_N,
            transb: cublasOperation_t::CUBLAS_OP_T,
            m: od as i32,
            n: rows as i32,
            k: in_dim as i32,
            alpha: 1.0,
            lda: in_dim as i32,
            ldb: in_dim as i32,
            beta: 0.0,
            ldc: od as i32,
        };
        {
            // write into the column slice of out
            let off = out_col_offset;
            let mut cview = out.slice_mut(off..off + od * rows);
            unsafe { self.blas.gemm(cfg, w, a, &mut cview)? };
        }
        // add bias broadcast: out[r, off+d] += bias[d]
        let total = rows * od;
        let width = od;
        let total_i = total as i32;
        let width_i = width as i32;
        let mut b = self.stream.launch_builder(&self.fns.add_bias);
        {
            let mut cview = out.slice_mut(out_col_offset..out_col_offset + total);
            b.arg(&mut cview);
            b.arg(bias);
            b.arg(&total_i);
            b.arg(&width_i);
            unsafe { b.launch(launch_cfg(total as u32, 256))? };
        }
        Ok(())
    }
}

fn launch_cfg(total: u32, block: u32) -> LaunchConfig {
    let blocks = total.div_ceil(block).max(1);
    LaunchConfig {
        grid_dim: (blocks, 1, 1),
        block_dim: (block, 1, 1),
        shared_mem_bytes: 0,
    }
}

impl Embedder for CudaEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let encs = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow!("tokenize: {e}"))?;
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
        let hidden = self.forward(n, width, &ids, &mask)?;
        // mean-pool + L2 on host (CPU cost negligible vs 12 encoder layers)
        let h = self.weights.h;
        let mut out = Vec::with_capacity(n);
        for r in 0..n {
            let mut acc = vec![0.0f32; h];
            let mut count = 0.0f32;
            for c in 0..width {
                if mask[r * width + c] == 0 {
                    continue;
                }
                let src = (r * width + c) * h;
                for (a, &xx) in acc.iter_mut().zip(&hidden[src..src + h]) {
                    *a += xx;
                }
                count += 1.0;
            }
            let count = count.max(1.0);
            let norm = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            for a in acc.iter_mut() {
                *a /= count;
            }
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
