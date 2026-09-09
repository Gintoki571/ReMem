# Latency amortization: persistent MCP vs per-call CLI spawns

Question: MCP fuzz showed ~1 op/s bulk throughput under a per-process
model-load assumption. Does one persistent `remem-mcp` stdio server
amortize that away?

Method: scratch DBs, debug binaries (`cargo build` green 2026-09-08;
sibling mid-edit in `remem-recall`, binaries current at time of test),
20 remember + 20 recall ops (40 total) each mode, CPU embedder 768d.

| mode | first-op | steady-state avg | total (40 ops) |
|------|----------|------------------|----------------|
| persistent MCP, 1 stdio session | 1.78 s (remember) / 1.31 s (recall) | 1.52 s/op | 61.0 s |
| CLI, 40 separate spawns | 1.48 s | 1.53 s/op | 61.3 s |

Server spawn-to-first-response (incl. model load): 0.2 s.

Verdict: NO — persistent MCP does not solve the ~1 op/s finding.
Model load (~0.2 s, once) is negligible; per-op embedding inference on
CPU (~1.5 s) dominates and is identical in both modes. Keep-alive saves
~0.3 s total over 40 ops (noise).

Implication for prime-agent wiring: keep one server alive for session
hygiene (fewer spawns), NOT for speed; real throughput wins need faster
inference (GPU/cached embeddings), not connection reuse.
