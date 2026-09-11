# GPU CUDA backend — lane-gpu (issue #5)

Branch: `lane-gpu` (base 92f421a). Machine: GTX 1660 Ti (sm_75), CUDA 13.3, driver 610.57.04.

## 1. candle-kernels failure (reproduced)

Command: `TMPDIR=$PWD/tmp-build cargo check -p remem-embed --features cuda` (nvcc V13.3.73, candle-kernels 0.11.0). Exit 101.

```
src/compatibility.cuh(11): error: function "__hmax_nan(__half, __half)" has already been
  defined (previous definition at line 3309 of .../include/cuda_fp16.hpp)
src/compatibility.cuh(14): error: function "__hmin_nan(__half, __half)" has already been
  defined (previous definition at line 3326 of .../include/cuda_fp16.hpp)
2 errors detected in the compilation of "src/affine.cu" (and conv, sort, cast, indexing, binary)
Error: CompilationFailed { path: "src/affine.cu", message: "nvcc error" }
```

Matches docs/cuda-unblock.md: candle-kernels 0.11.0 defines `__hmax_nan`/`__hmin_nan` under
`#if __CUDA_ARCH__ < 800` (true on sm_75); CUDA 13.x `cuda_fp16.hpp` defines both unconditionally.
Upstream fix: PR huggingface/candle#3909 (unmerged as of 2026-09-10).

## 2. cuda backend design

**Chosen: cudarc (nvrtc PTX + cublas sgemm), not candle-cuda, not hand-written cubin.**

1. candle-cuda is blocked by candle-kernels vs CUDA 13.3/sm_75 (see §1); patching candle-kernels
   or a CUDA 12.6 side-install is env surgery this lane cannot own.
2. cudarc 0.19.9 is already in Cargo.lock (via candle), needs no new dependency, and does NOT run
   nvcc on any candle kernel — it compiles small PTX strings with libnvrtc at runtime.
3. BERT forward needs only: cublas sgemm/strided-batched (all matmuls) + 7 tiny f32 kernels
   (layernorm, gelu, elementwise add, add-bias, masked softmax, 2 head permutes). ~400 lines of
   Rust + ~120 lines of CUDA-in-string, far less than porting candle's kernel suite.

Design:
- `crates/remem-embed/src/cuda.rs` (feature `cuda` = `dep:cudarc`; candle cuda features removed
  from the feature — the documented `--features cuda` flag now builds the working path).
- Weights: manual safetensors parse (8-byte len + serde_json header + f32 blob), upload once.
- Forward: f32 BERT encoder, batch-major, attention via `gemm_strided_batched` with the
  row-major recipe `C[M,N] = A[M,K]·Bop` → cublas(m=N,n=M,k=K,opN,opN, B-slot=A, ldb=K, ldc=N).
  Attention scale folded into gemm alpha (1/sqrt(hk)).
- Pooled mean + L2 on host after final hidden copy (CPU cost negligible vs 12 layers).
- Runtime fallback: `load()` tries CUDA when built with `cuda`, on ANY error logs and returns the
  candle CPU embedder. Without the feature, pure candle CPU (unchanged default).

## 2b. RED status

PENDING.

## 3. Bench (768d batch, GPU vs CPU)

PENDING.

## 4. Deprecation

PENDING.

## Friction notes (dogfood)

PENDING.
