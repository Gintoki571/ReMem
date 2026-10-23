//! Throwaway calibration: description-conflict threshold for entity homonyms.
//! Run with the real model: `cargo run --release --example measure_desc`.

fn main() {
    let emb = neocortex::embed::load(None).expect("model");
    let same = [
        ("backend engineer at Stripe in Boston", "works as a backend engineer for Stripe, lives in Boston"),
        ("the user's mother", "my mom"),
        ("a bakery owner in Chicago", "runs a bakery in Chicago"),
        ("Alice from accounting, likes jazz", "Alice in the accounting team; jazz fan"),
        ("project manager for the Atlas project", "manages the Atlas project"),
        ("CTO of Acme since 2020", "chief technology officer at Acme"),
        ("lives in Pune with her family", "based in Pune, family in Pune"),
        ("a database administrator", "DBA who administers databases"),
        ("German shepherd named Rex", "dog Rex, a German shepherd"),
        ("favourite restaurant in Delhi", "a Delhi restaurant the user likes best"),
        ("used to work at IBM", "former IBM employee"),
        ("the client's CFO", "chief financial officer at the client"),
        ("Bob's sister", "the sister of Bob"),
        ("a small startup in Berlin", "Berlin-based startup, small team"),
        ("the person who runs the deploy script", "owns the deployment automation"),
        ("Maria from the design team", "designer Maria"),
        ("the team's manager, reports to the VP", "manages the team and reports to the VP"),
        ("a Rust developer", "programmer who writes Rust"),
        ("landlord of the flat", "owns the apartment we rent"),
        ("the new CTO, started in March", "CTO since March, newly hired"),
        ("an ER nurse in Oslo", "nurse working in an Oslo emergency room"),
        ("the user's landlord", "the person who rents us the flat"),
    ];
    let different = [
        ("backend engineer at Stripe in Boston", "runs a bakery in Chicago"),
        ("the user's mother", "a bakery owner in Chicago"),
        ("Alice from accounting, likes jazz", "Alice the marathon runner"),
        ("CTO of Acme since 2020", "a marine biologist in Perth"),
        ("lives in Pune with her family", "a bakery owner in Chicago"),
        ("a database administrator", "a primary school teacher"),
        ("German shepherd named Rex", "a tabby cat"),
        ("favourite restaurant in Delhi", "a jazz musician from New Orleans"),
        ("used to work at IBM", "an ER nurse in Oslo"),
        ("the client's CFO", "a freelance photographer"),
        ("Bob's sister", "a bakery owner in Chicago"),
        ("a small startup in Berlin", "a retired schoolteacher"),
        ("the person who runs the deploy script", "a pastry chef"),
        ("Maria from the design team", "a structural engineer"),
        ("the team's manager, reports to the VP", "a beekeeper in Vermont"),
        ("a Rust developer", "a wedding planner"),
        ("landlord of the flat", "a scuba instructor"),
        ("the new CTO, started in March", "a violinist in the city orchestra"),
        ("a backend engineer at Stripe in Boston", "the mayor of a small town"),
        ("a bakery owner in Chicago", "a backend engineer at Stripe in Boston"),
        ("an ER nurse in Oslo", "a violinist in the city orchestra"),
        ("the user's landlord", "a marine biologist in Perth"),
    ];
    let mut texts: Vec<String> = Vec::new();
    for (a, b) in same.iter().chain(different.iter()) {
        texts.push(format!("Alex (person): {a}"));
        texts.push(format!("Alex (person): {b}"));
    }
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let vectors = emb.embed(&refs, neocortex::embed::Mode::Document).unwrap();
    let cosine = |a: &[f32], b: &[f32]| -> f32 { a.iter().zip(b).map(|(x, y)| x * y).sum() };
    let mut same_scores = Vec::new();
    let mut diff_scores = Vec::new();
    for (i, (a, b)) in same.iter().enumerate() {
        let score = cosine(&vectors[i * 2], &vectors[i * 2 + 1]);
        same_scores.push(score);
        println!("same  {score:.4}  {a} | {b}");
    }
    let offset = same.len();
    for (i, (a, b)) in different.iter().enumerate() {
        let score = cosine(&vectors[(offset + i) * 2], &vectors[(offset + i) * 2 + 1]);
        diff_scores.push(score);
        println!("diff  {score:.4}  {a} | {b}");
    }
    let stats = |v: &[f32]| {
        let min = v.iter().copied().fold(f32::INFINITY, f32::min);
        let max = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        (min, max, mean)
    };
    println!("\nsame n={} min/max/mean = {:.4}/{:.4}/{:.4}", same_scores.len(), stats(&same_scores).0, stats(&same_scores).1, stats(&same_scores).2);
    println!("diff n={} min/max/mean = {:.4}/{:.4}/{:.4}", diff_scores.len(), stats(&diff_scores).0, stats(&diff_scores).1, stats(&diff_scores).2);
    println!("\nthreshold sweep (conflict if cosine < t):");
    for t in [0.30f32, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75] {
        let tp = diff_scores.iter().filter(|s| **s < t).count();
        let fp = same_scores.iter().filter(|s| **s < t).count();
        let precision = if tp + fp == 0 { 1.0 } else { tp as f32 / (tp + fp) as f32 };
        let recall = tp as f32 / diff_scores.len() as f32;
        println!("t={t:.2} precision={precision:.3} recall={recall:.3} (fp={fp}, tp={tp})");
    }
}
