// RAYON_NUM_THREADS sweep (i7-9750H, 2026-09-09): 1->10.9s 2->5.9s 4->2.2s 8->1.7s per op.
// Rayon defaults to all cores, so no in-code setting beats default; run with
// RAYON_NUM_THREADS only to constrain. No code change from this bench.
use remem_embed::Embedder;
fn main() {
    let threads = candle_core::utils::get_num_threads();
    let m = remem_embed::load().expect("load");
    let text = "The quick brown fox jumps over the lazy dog near the river bank.";
    let _ = m.embed(&[text]).unwrap(); // warmup
    let t0 = std::time::Instant::now();
    for _ in 0..10 {
        let _ = m.embed(&[text]).unwrap();
    }
    let dt = t0.elapsed();
    println!("threads={} total_10x={:.2}s per_op={:.3}s", threads, dt.as_secs_f64(), dt.as_secs_f64() / 10.0);
}
