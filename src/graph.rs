//! Graph algorithms over an in-memory CSR snapshot of the knowledge graph:
//! approximate personalized PageRank (Andersen-Chung-Lang push). The
//! graphqlite projection and adjacency loading live in `kg.rs`.

/// Undirected weighted graph in CSR form. Node ids are dense 0..n.
pub struct Csr {
    pub offsets: Vec<usize>,
    pub targets: Vec<u32>,
    pub weights: Vec<f32>,
    /// Weighted degree (sum of incident weights) per node.
    pub wdeg: Vec<f64>,
}

impl Csr {
    /// Build from (a, b, w) pairs; each pair is added in both directions,
    /// duplicate pairs are merged by summing weights, self-loops and w <= 0
    /// are dropped.
    pub fn from_edges(n: usize, edges: &[(u32, u32, f32)]) -> Csr {
        todo!("lane graph")
    }

    pub fn len(&self) -> usize {
        self.wdeg.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wdeg.is_empty()
    }
}

/// Personalized PageRank by ACL push (non-lazy):
/// p = alpha * s + (1 - alpha) * p * D^-1 W, where s = seeds normalised to
/// sum 1. Push node u while r(u) >= eps * wdeg(u); nodes with wdeg 0 keep
/// their residual (act as self-loops). Stops after max_pushes pushes.
/// Returns (node, score) with score > 0, sorted descending.
pub fn ppr_push(g: &Csr, seeds: &[(u32, f64)], alpha: f64, eps: f64, max_pushes: usize) -> Vec<(u32, f64)> {
    todo!("lane graph")
}

/// Exact PPR by power iteration with the same semantics as ppr_push
/// (reference for tests).
pub fn ppr_power(g: &Csr, seeds: &[(u32, f64)], alpha: f64, iters: usize) -> Vec<f64> {
    todo!("lane graph")
}
