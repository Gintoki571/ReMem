use cudarc::cublas::safe::{CudaBlas, Gemm, GemmConfig};
use cudarc::cublas::sys::cublasOperation_t;
use cudarc::driver::{CudaContext, CudaSlice};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let ctx = CudaContext::new(0)?;
    let stream = ctx.default_stream();
    let blas = CudaBlas::new(stream.clone())?;
    let w: CudaSlice<f32> = stream.clone_htod(&vec![0.5f32; 768 * 768])?;
    let a: CudaSlice<f32> = stream.clone_htod(&vec![1.0f32; 9 * 768])?;
    let mut c: CudaSlice<f32> = stream.alloc_zeros::<f32>(9 * 768)?;
    let cfg = GemmConfig::<f32> {
        transa: cublasOperation_t::CUBLAS_OP_T,
        transb: cublasOperation_t::CUBLAS_OP_N,
        m: 768,
        n: 9,
        k: 768,
        alpha: 1.0,
        lda: 768,
        ldb: 768,
        beta: 0.0,
        ldc: 768,
    };
    let _ = unsafe { blas.gemm(cfg, &w, &a, &mut c) };
    stream.synchronize()?;
    let t = Instant::now();
    for _ in 0..1000 {
        unsafe { blas.gemm(cfg, &w, &a, &mut c)? };
    }
    stream.synchronize()?;
    println!(
        "1000 sgemm: {:?} -> {:?}/gemm",
        t.elapsed(),
        t.elapsed() / 1000
    );
    // kernel launch cost
    Ok(())
}
