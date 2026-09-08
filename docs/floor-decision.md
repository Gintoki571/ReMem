# --min-score floor decision (issue #1)

Measured on v3 (608eb79), debug build, fresh db, all 40 fixtures loaded,
37 answerable + 3 adversarial queries, k=5.

| min-score | recall@1 | recall@5 | adversarial (want []) |
|-----------|----------|----------|------------------------|
| 0         | 31/37    | 37/37    | 0/3                    |
| 0.02      | 31/37    | 35/37    | 3/3                    |

## Q27 / Q28

Tag boost did not lift them above the floor.

| Query | Target rank @0 | Target score | Fate @0.02 |
|-------|----------------|--------------|------------|
| Q27 "wait what did we decide at the start of the year about spending" | 5 | 0.0165 | dropped |
| Q28 "did the auditors ever get back to us about that winter check"    | 3 | 0.0159 | dropped |

## Adversarial behavior at 0

Junk queries ("what is the thing about stuff" etc.) return top-k noise at
scores 0.0146-0.0156. Q27/Q28 targets sit at 0.0159-0.0165.

## Recommendation

Stay off (default 0). Keep --min-score opt-in.

Reason: true targets (0.0159+) and junk (up to 0.0156) overlap almost exactly,
so any floor that suppresses the noise costs real answers -- the 0.02 floor
trades two true hits (Q27, Q28) for noise suppression, and a floor tuned
between them (e.g. 0.0157) would be overfit to these fixtures by a 0.0003
margin.
