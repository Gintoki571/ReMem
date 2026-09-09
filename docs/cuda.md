# CUDA / GPU embedding (crates/remem-embed)

Status: **GPU build broken on this machine** (2026-09-09). CPU path is the default and works.

## Machine

- GPU: GTX 1660 Ti (Turing, sm_75), driver 610.57.04
- Toolkit: CUDA 13.3 (`/opt/cuda`, nvcc V13.3.73), Arch Linux `cuda 13.3.1-1`
- candle 0.11.0 / candle-kernels 0.11.0 / cudarc 0.19.8

## What works

- `cargo test -p remem-embed` (CPU, default features): 4 passed, 1 ignored (`gpu_embed_smoke`).
- cublas/cublasLt present: `/opt/cuda/lib64/libcublas.so.13` (ldconfig finds it).
- cudnn is NOT installed and is NOT needed: remem-embed's `cuda` feature enables
  `candle-core/cuda`, which pulls cudarc with `cublas,cublaslt,curand,driver,nvrtc`
  (dynamic linking) — no cudnn. cudnn would only be pulled by candle's separate
  `cudnn` feature, which remem-embed does not use. Missing libcudnn is not the blocker.

## What fails

`cargo check -p remem-embed --features cuda` (exit 101) fails in `candle-kernels`' build
script, compiling its CUDA kernels with nvcc 13.3:

    src/compatibility.cuh(11): error: function "__hmax_nan(__half, __half)" has already been
    defined (previous definition at line 3309 of /opt/cuda/.../include/cuda_fp16.hpp)
    src/compatibility.cuh(14): error: function "__hmin_nan(__half, __half)" has already been
    defined (previous definition at line 3326 of ...)

Root cause: `candle-kernels-0.11.0/src/compatibility.cuh` defines `__hmax_nan`/`__hmin_nan`
under `#if __CUDA_ARCH__ < 800` (true for sm_75). CUDA 13.x's `cuda_fp16.hpp` now defines
both unconditionally (via NV_IF_ELSE_TARGET), so on any sub-sm_80 arch with CUDA 13 the
definitions collide. Upstream candle `main` has the same code (unfixed); candle CI only
exercises sm_80+ so it does not catch this.

## Fix options (pick one)

1. Build with a CUDA 12.x toolkit (least code, no repo change). Install side-by-side
   without touching system packages:

       wget https://developer.download.nvidia.com/compute/cuda/12.6.3/local_installers/cuda_12.6.3_560.35.05_linux.run
       sh cuda_12.6.3_560.35.05_linux.run --silent --toolkit --toolkitpath=$HOME/cuda-12.6

   Then build/test with the 12.x toolkit:

       CUDA_HOME=$HOME/cuda-12.6 cargo check -p remem-embed --features cuda
       CUDA_HOME=$HOME/cuda-12.6 cargo test -p remem-embed --features cuda -- --ignored gpu_embed_smoke

   Note `cuda-version-from-build-system` in cudarc: it links against the toolkit that
   nvcc reports, so with a 12.x prefix the runtime must find the 12.x libs:

       export LD_LIBRARY_PATH=$HOME/cuda-12.6/lib64:$LD_LIBRARY_PATH

2. Fork candle-kernels and guard the two definitions with the toolkit version, then add
   to the workspace `Cargo.toml` (outside remem-embed, needs maintainer sign-off):

       [patch.crates-io]
       candle-kernels = { git = "https://github.com/<you>/candle", branch = "cuda13-half-fix" }

   Patch = wrap both functions in `#if !defined(__CUDACC_VER_MAJOR__) || __CUDACC_VER_MAJOR__ < 13`.
   Do NOT force `-arch=compute_80`: PTX for sm_80 will not JIT on sm_75.

## Environment summary (for a working CUDA build)

- Build: `CUDA_HOME` must point at a toolkit whose version is compatible with
  candle-kernels (<= 12.x as of candle 0.11.0).
- Run: `LD_LIBRARY_PATH` must contain that toolkit's `lib64` (dynamic cudarc linking);
  current shell already has `/opt/cuda/lib64`.

## Timings CPU vs GPU

N/A — GPU build blocked (see above). Re-measure after the fix:

    cargo test -p remem-embed --features cuda -- --ignored gpu_embed_smoke --nocapture
