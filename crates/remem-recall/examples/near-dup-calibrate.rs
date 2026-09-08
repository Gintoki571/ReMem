/// Calibrate `SIMILAR_MAX_DISTANCE` (the `remember` near-duplicate probe).
///
/// Embeds every memory in docs/eval-fixtures.json with the real local
/// embedder and prints L2 distances (vectors are L2-normalized, so
/// d = sqrt(2 - 2 cos)) for the fixture's deliberate paraphrase pairs versus
/// the closest non-paraphrase pairs. Run:
///   cargo run -p remem-recall --example near-dup-calibrate
use remem_embed::Embedder as _;

/// Fixture indices the eval wrote as re-phrasings of each other
/// (docs/eval-fixtures.json: 15/32, 8/33, 21/34, 22/35).
const PARAPHRASES: [(usize, usize); 4] = [(15, 32), (8, 33), (21, 34), (22, 35)];

fn l2(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

fn main() -> anyhow::Result<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/eval-fixtures.json");
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let contents: Vec<&str> = fx["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap())
        .collect();
    let m = remem_embed::load()?;
    println!("embedder: {} ({}d)", m.device_name(), m.dims());
    let vecs = m.embed(&contents)?;

    let mut para: Vec<(f32, String)> = Vec::new();
    let mut other: Vec<(f32, String)> = Vec::new();
    for i in 0..contents.len() {
        for j in i + 1..contents.len() {
            let d = l2(&vecs[i], &vecs[j]);
            let label = format!(
                "{i}/{j} {d:.4}\n     a: {}\n     b: {}",
                contents[i], contents[j]
            );
            if PARAPHRASES.contains(&(i, j)) || PARAPHRASES.contains(&(j, i)) {
                para.push((d, label));
            } else {
                other.push((d, label));
            }
        }
    }
    para.sort_by(|a, b| a.0.total_cmp(&b.0));
    other.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\n== paraphrase pairs (want FIRE) ==");
    for (_, l) in &para {
        println!("{l}");
    }
    println!("\n== 10 closest NON-paraphrase pairs (want SILENT) ==");
    for (_, l) in other.iter().take(10) {
        println!("{l}");
    }
    println!(
        "\nmax paraphrase d = {:.4}   min other d = {:.4}",
        para.iter().map(|p| p.0).fold(0.0f32, f32::max),
        other[0].0
    );
    Ok(())
}
