use candle_core::Device;
use remem_embed::{load, load_candle, model_dir, Embedder};
use std::time::Instant;

fn bench(name: &str, m: &dyn Embedder, texts: Vec<String>, iters: usize) {
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let _ = m.embed(&refs).unwrap();
    let t = Instant::now();
    for _ in 0..iters {
        let v = m.embed(&refs).unwrap();
        std::hint::black_box(v);
    }
    println!(
        "{name} n={}: {:?}/batch ({} iters)",
        texts.len(),
        t.elapsed() / iters as u32,
        iters
    );
}

fn main() {
    let gpu = load().unwrap();
    let cpu = load_candle(&model_dir(), Device::Cpu).unwrap();
    let base = [
        "The cat sat on the mat and watched the rain fall softly outside.",
        "Retrieval augmented generation combines search with language models.",
        "Rust systems programming offers memory safety without garbage collection.",
        "The quick brown fox jumps over the lazy dog near the riverbank.",
    ];
    let four: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    let one: Vec<String> = vec![base[0].to_string()];
    let big: Vec<String> = (0..32).map(|i| base[i % 4].to_string()).collect();
    bench("GPU", &gpu, four.clone(), 30);
    bench("CPU", &cpu, four.clone(), 10);
    bench("GPU", &gpu, one.clone(), 30);
    bench("CPU", &cpu, one.clone(), 10);
    bench("GPU", &gpu, big.clone(), 10);
    bench("CPU", &cpu, big.clone(), 3);
}
