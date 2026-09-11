//! Pinned adversarial margin (doubt-scratchpad item 4): the junk pre-boost
//! ceiling must stay below the shipped recall floor.
//!
//! docs/eval.md ("Floor re-measurement post-hybrid") establishes that the
//! junk ceiling is structural, not corpus luck: a junk hit (content-free
//! query) gets no FTS match, so its best possible pre-boost fused score is
//! the vector-only RRF arm at rank 1, `Weights::default().vector /
//! (DEFAULT_RRF_K + 1)` = 0.5/31 = 0.016129 at the landed weights. The
//! shipped floor clears that ceiling by only ~5% (0.017 vs 0.016129), so any
//! future k / vector-weight / floor recalibration that eats the margin would
//! silently re-admit adversarial junk. This test pins the relationship so
//! such a recalibration fails loudly instead.

use remem_recall::{rank, Weights, DEFAULT_MIN_SCORE, DEFAULT_RRF_K};

/// The floor shipped against adversarial junk: `scripts/eval.sh` runs every
/// recall with `--min-score 0.017`. `DEFAULT_MIN_SCORE` itself stays 0.0
/// (off) per docs/floor-decision.md, so when the default is off this eval
/// floor is the operative one. If the eval.sh floor is ever recalibrated,
/// update this constant in the same commit.
const EVAL_FLOOR: f64 = 0.017;

/// Minimum relative margin the floor must hold over the junk ceiling.
/// Measured: (0.017 - 0.016129) / 0.016129 = 5.4%; docs describe the margin
/// as "~5%" (doubt-scratchpad item 4), so 5% is the pin.
const MIN_RELATIVE_MARGIN: f64 = 0.05;

#[test]
fn junk_preboost_ceiling_stays_below_shipped_floor_with_margin() {
    // Junk ceiling, built through the shipped fusion path: a single vector
    // list at its default weight, fused at the default k. The rank-1 entry
    // scores weight * 1/(k + 1) — the exact arm a no-FTS junk hit rides.
    let ids: Vec<String> = (0..4).map(|i| format!("m{i}")).collect();
    let vector_only = rank::Ranking {
        name: "vector",
        ids: &ids,
        weight: Weights::default().vector,
    };
    let fused = rank::fuse(&[vector_only], DEFAULT_RRF_K);
    assert_eq!(fused[0].id, "m0", "vector#1 must be the top fused entry");
    let junk_ceiling = fused[0].score;

    // The construction must equal the documented formula 0.5/(k+1): if
    // fuse/rrf ever changes shape, this forces the ceiling here to be
    // reinterpreted, not silently re-scaled.
    let formula = Weights::default().vector / (DEFAULT_RRF_K as f64 + 1.0);
    assert!((junk_ceiling - formula).abs() < 1e-12);

    // Operative floor: the crate default when it is on, else the eval floor.
    let floor = if DEFAULT_MIN_SCORE > 0.0 {
        DEFAULT_MIN_SCORE
    } else {
        EVAL_FLOOR
    };

    // Strictly below the floor...
    assert!(
        junk_ceiling < floor,
        "junk pre-boost ceiling {junk_ceiling:.6} is not below the shipped floor {floor:.6}: \
         adversarial junk would clear the floor. Re-calibrate the floor together with any \
         k/weight change (docs/eval.md 'Floor re-measurement post-hybrid', doubt-scratchpad \
         item 1)"
    );
    // ...and with at least the pinned relative margin.
    let margin = floor - junk_ceiling;
    assert!(
        margin >= junk_ceiling * MIN_RELATIVE_MARGIN,
        "floor {floor:.6} clears the junk ceiling {junk_ceiling:.6} by only {margin:.6} \
         ({:.1}% relative, pin is >= {:.0}%): the thin adversarial margin is eaten. Re-measure \
         the floor before landing (doubt-scratchpad item 4)",
        margin / junk_ceiling * 100.0,
        MIN_RELATIVE_MARGIN * 100.0
    );
}
