use neocortex::embed::{load, Mode};
use std::time::{Duration, Instant};

fn best(times: &[Duration]) -> Duration {
    *times.iter().min().unwrap()
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

fn main() {
    let start = Instant::now();
    let model = load(None).expect("load UltraBERT model");
    let mode = model.name();
    println!("{mode:16} load: {:.3}s", start.elapsed().as_secs_f64());

    let query = ["which database is production on"];
    for _ in 0..3 {
        model.embed(&query, Mode::Query).expect("warmup");
    }
    let mut samples = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        model.embed(&query, Mode::Query).expect("embed query");
        samples.push(start.elapsed());
    }
    let best_query = best(&samples);
    println!(
        "{mode:16} query median (20): {}  best: {}",
        format_duration(median(&mut samples)),
        format_duration(best_query)
    );

    let docs = vec!["short document about production databases"; 64];
    let mut runs = Vec::with_capacity(3);
    for _ in 0..3 {
        let start = Instant::now();
        model.embed(&docs, Mode::Document).expect("embed documents");
        runs.push(start.elapsed());
    }
    let best_docs = best(&runs);
    println!(
        "{mode:16} documents: 64 in {} ({:.1} docs/s)",
        format_duration(best_docs),
        64.0 / best_docs.as_secs_f64()
    );

    let long_text = "the production database migration ran on the primary host while the \
        replica lagged behind by several minutes and the monitoring system paged the on call \
        engineer who restarted the connection pool and cleared the stale cache entries before \
        the nightly backup job started writing to the archive volume on the storage cluster \
        which had been resized last week after the capacity review meeting concluded that the \
        growth rate of the index tables would exhaust the remaining free space within six weeks"
        .repeat(5);
    let long = [long_text.as_str()];
    let mut runs = Vec::with_capacity(3);
    for _ in 0..3 {
        let start = Instant::now();
        let out = model.embed(&long, Mode::Document).expect("embed long");
        assert_eq!(out.len(), 1);
        runs.push(start.elapsed());
    }
    println!("{mode:16} long doc: {}", format_duration(best(&runs)));
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.3}s", duration.as_secs_f64())
    } else {
        format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
    }
}
