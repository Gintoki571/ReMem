# CUDA / GPU embedding (crates/remem-embed)

Status: **WORKING** (lane-gpu). `cargo test -p remem-embed --features cuda` passes on-device;
the `cuda` feature uses a hand-rolled cudarc backend that bypasses candle-kernels entirely.

## Machine

- GPU: GTX 1660 Ti (Turing, sm_75), driver 610.57.04
- Toolkit: CUDA 13.3 (`/opt/cuda`, nvcc V13.3.73)
- cudarc 0.19.9, feature pin `cuda-13030` (required — cudarc must match the toolkit
  version or its build script fails); dynamic linking, no cudnn, no nvcc at build time
  beyond feature selection (PTX is compiled at runtime via libnvrtc).

## Why not candle-cuda

candle-kernels 0.11 cannot compile against CUDA 13.x on sm_75 (`__hmax_nan`/`__hmin_nan`
redefinition, docs/cuda-unblock.md). The `cuda` feature here does NOT enable
`candle-core/cuda`; instead `src/cuda.rs` implements the BERT forward directly on cudarc:
cublas sgemm for all projections + small custom PTX kernels (nvrtc at load time,
`compute_75`) for layernorm, masked softmax, gelu, bias add, residual add, embedding
gather, mean-pool, L2 rows.

## Design notes (hard-won, keep)

- BERT here is **post-LN**: `x = LN(x + attn_out)`, `x = LN(x + ffn_out)`. LN weights are
  `attention.output.LayerNorm` / `output.LayerNorm`; do not "optimize" into pre-norm.
- proj layout: three dense `[n*w, h]` row-major blocks (q at 0, k at `n*w*h`, v at
  `2*n*w*h`). gemm_bias writes dense blocks; earlier interleaved q|k|v per-row layout
  was the source of silent garbage.
- cublas operand orientation (col-major): for row-major `W [od, id]` and row-major
  `A [rows, id]`, use `transa=T lda=id, transb=N ldb=id, C ldc=od` — element (d, r) at
  `r*od + d` equals row-major `out[r, d]`.
- **cublas tt-kernel bug**: `transa=T, transb=T` with `k=3072` (any m/n) on sm_75 +
  CUDA 13.3 faults (`volta_sgemm_128x32_tt` reads OOB, 100s of invalid accesses).
  `transb=N` computes the identical values for our layout and is used everywhere.
- softmax mask is per-document u32 (`1` = keep); attention scores are masked by adding
  `-1e9` for mask==0 tokens. The kernel takes `const unsigned int*` — reading u32 mask
  bits as f32 yields ~0 and silently masks every token (uniform 1/9 attention).
- Mean-pool then L2: normalize AFTER dividing by count, not before (dividing the sum by
  count after normalizing the sum scales the result by 1/count).

## Verified

- `cargo test -p remem-embed` (CPU): 8 passed, 1 ignored.
- `cargo test -p remem-embed --features cuda`: 8 passed + 4 cuda tests on device
  (load/dims, L2 norm, GPU-vs-CPU cosine > 0.999, empty batch).
- GPU vs CPU reference (numpy) layer-0 activations match to fp32 rounding; final
  embeddings match CPU candle to cosine 0.99999+ including mixed-length padded batches.

## Timings (release, `examples/bench`)

**Caveat: this GPU is power-capped to 10 W of 80 W (P8, SW power/thermal slowdown
active) and cannot be raised without root (`nvidia-smi -pl 80`). Every CUDA launch
costs ~0.9 ms under this cap, which dominates; the numbers below are launch-bound,
not compute-bound.**

| batch | GPU (cudarc) | CPU (candle) |
|-------|--------------|--------------|
| 1     | 254 ms       | 86 ms        |
| 4     | 420 ms       | 169 ms       |
| 32    | 2.62 s       | 0.93 s       |

Per-layer time is a flat ~17.5 ms regardless of batch size — pure launch overhead
(~100 launches/embed). With a normal power limit, expect the GPU path to win by a
large margin; re-bench after `nvidia-smi -pl 80` and removing the cap.

## Deprecation

The cudarc backend is a stopgap. Remove it (and the `cuda` feature) once candle ships
huggingface/candle#3909 (see docs/cuda-unblock.md re-check procedure) — then switch
`load()` back to `Device::cuda_if_available` and candle-kernels.
