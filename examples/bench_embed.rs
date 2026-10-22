use neocortex::embed::{load, Mode};
use std::time::{Duration, Instant};

fn main() {
    let start = Instant::now();
    let model = load(None).expect("load UltraBERT model");
    println!("load: {:.3}s", start.elapsed().as_secs_f64());

    let query = ["which database is production on"];
    let mut samples = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        model.embed(&query, Mode::Query).expect("embed query");
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    println!("query median (20): {}", format_duration(samples[10]));

    let docs = vec!["short document about production databases"; 64];
    let start = Instant::now();
    model.embed(&docs, Mode::Document).expect("embed documents");
    let elapsed = start.elapsed();
    println!(
        "documents: 64 in {:.3}s ({:.1} docs/s)",
        elapsed.as_secs_f64(),
        64.0 / elapsed.as_secs_f64()
    );
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.3}s", duration.as_secs_f64())
    } else {
        format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
    }
}
