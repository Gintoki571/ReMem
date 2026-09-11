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

PENDING.

## 3. Bench (768d batch, GPU vs CPU)

PENDING.

## 4. Deprecation

PENDING.

## Friction notes (dogfood)

PENDING.
