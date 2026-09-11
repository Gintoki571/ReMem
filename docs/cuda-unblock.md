# CUDA unblock status — candle-kernels `__hmax_nan`/`__hmin_nan` redefinition

Re-checked 2026-09-10 against upstream. **Still unfixed. Do not bump candle.**

## Status

- Latest candle-kernels release: **0.11.0** (crates.io, published 2026-06-26). No release
  contains a fix.
- `main` branch `candle-kernels/src/compatibility.cuh` still defines `__hmax_nan`/`__hmin_nan`
  under `#if __CUDA_ARCH__ < 800` (last commit touching the file: #3558, 2026-05-26), which
  collides with CUDA 12.2+/13.x `cuda_fp16.hpp` on sm_75.
- Fix exists as open PR **huggingface/candle#3909** ("gate __hmax_nan/__hmin_nan shim on
  toolkit version", opened 2026-09-02, not yet reviewed/merged). It guards with
  `CANDLE_CUDA_VERSION < 12020 && __CUDA_ARCH__ < 800`.

## Re-check trigger

Bump candle only when #3909 is merged AND a candle release newer than 0.11.0 ships it:

    gh pr view 3909 --repo huggingface/candle --json state,mergedAt
    curl -s https://crates.io/api/v1/crates/candle-kernels | grep -o '"newest_version":"[^"]*"'

If merged but unreleased, use the PR author's fork as a `[patch.crates-io]` candle-kernels
source (see docs/cuda.md option 2) — do not bump the released version.

## Until then

The lane-gpu cudarc backend (docs/cuda.md) is the active workaround on this machine; the
options below remain the alternatives if a candle-native path is preferred:

1. Side-by-side CUDA 12.x toolkit: `CUDA_HOME=$HOME/cuda-12.6 cargo check -p remem-embed --features cuda`
   (plus `LD_LIBRARY_PATH=$HOME/cuda-12.6/lib64:$LD_LIBRARY_PATH` at runtime).
2. Fork/patch candle-kernels guarding the shim with `__CUDACC_VER_MAJOR__ < 13`
   (PR #3909's `< 12020` guard is the more correct form to copy).

## cudnn

Not needed. `remem-embed`'s `cuda` feature pulls candle-core/cuda → cudarc with
`cublas,cublaslt,curand,driver,nvrtc` only. cudnn requires candle's separate `cudnn`
feature, which remem-embed does not enable. Unchanged from docs/cuda.md.
