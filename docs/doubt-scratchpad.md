# Doubt scratchpad (issue #12, lane F)

Minimal review queue: doubts that survived the v3.1 spike round, in priority
order. Each item is a decision already made that someone should be able to
challenge with a number.

1. **RRF k and --min-score floor are coupled.** DEFAULT_RRF_K (30) and the
   shipped floor (0.017) are calibrated together; any k change is a pure
   score-scale change, so the floor must be retuned in the same commit.
   Measured: k=30 real dual hit 1.5/(k+1)=0.0484 vs junk vector-only
   0.5/(k+1)=0.0161 (junk barely under floor); k=120 puts the real hit at
   0.0124, under the floor — eval collapses to 0/37 while adversarial still
   "passes" vacuously. docs/weight-spike.md does not record this coupling.
2. **Insert-time kNN auto-link regressed every variant.** 3-NN at thresholds
   0.48/0.35/0.25 and a 1-NN variant all dropped recall@1 35 -> 27 with zero
   gained, and shipped adversarial 3/3 -> 1/3. Insensitive to threshold and
   degree: the fixture corpus near-dup clusters link under any cutoff, and
   graph weight 1.0 equals fts weight, so any edge into a cluster sibling
   outvotes a single-list top hit. Revisit only jointly with a graph-weight
   retune (< 0.5) or cross-topic-only linking.
3. **Every CLI invocation pays full embedder load** (~5-6s debug, ~16s first
   recall on the real db). No daemon/serve mode exists; a `remem serve` would
   amortize it. Decide when CLI latency actually blocks a workflow.
4. **Adversarial pass margin at the shipped floor is thin**: junk pre-boost
   0.0161 vs floor 0.017 at k=30 (~5%). Pinned by
   `crates/remem-recall/tests/pinned_margin.rs` (junk ceiling < floor with
   >= 5% relative margin), so a floor/k recalibration that eats the margin
   fails loudly.
