use remem_graph::Graph;
use remem_types::MemoryKind;
use std::time::Instant;

#[test]
#[ignore = "manual smoke: run with `cargo test -p remem-graph -- --ignored`"]
fn write_rate_is_sane() {
    let g = Graph::open_in_memory().unwrap();
    let t = Instant::now();
    for i in 0..1000 {
        g.upsert_memory(&format!("m{i}"), MemoryKind::Fact).unwrap();
        if i > 0 {
            g.link(&format!("m{i}"), &format!("m{}", i - 1), "RELATES_TO")
                .unwrap();
        }
    }
    let dur = t.elapsed();
    println!(
        "1000 nodes + 999 edges in {dur:?} ({} ops/s)",
        (2000f64 / dur.as_secs_f64()) as i64
    );
    let t = Instant::now();
    for _ in 0..1000 {
        g.neighbors("m500").unwrap();
    }
    println!("1000 neighbor reads in {:?}", t.elapsed());
}
