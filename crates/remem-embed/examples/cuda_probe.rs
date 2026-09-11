use remem_embed::{load, Embedder};

fn main() -> anyhow::Result<()> {
    let m = load()?;
    println!("{}", m.device_name());
    let vs = m.embed(&["The cat sat on the mat."])?;
    let v = &vs[0];
    println!(
        "len={} norm={}",
        v.len(),
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    );
    println!("first5={:?} last3={:?}", &v[..5], &v[v.len() - 3..]);
    Ok(())
}
