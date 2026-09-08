# MCP stdio server fuzz report (v3, debug binary)

Binary: `cargo build -p remem-mcp` (debug), built clean on 1st try — no staleness.
DB: `REMEM_DB=/tmp/remem-mcp-fuzz.db`. Method: one process per case, stdin bytes in,
stdout reply + exit + hang recorded. Embedder: real local model (`Cpu, 768d`).
**No crashes, no hangs, no panics in any case.** `exit=0` everywhere except case 4.

## Results

| Input | Reply / exit | Verdict |
|---|---|---|
| Garbage line (`hello this is not json`) | `-32700` invalid JSON, `id:null`, exit 0 | PASS |
| Empty line | no reply, exit 0 | PASS |
| Truncated JSON (`{... "method":`) | `-32700` EOF-while-parsing, exit 0 | PASS |
| Valid JSON non-object: `42`, `[1,2,3]`, `null`, `"hello"` | **no reply at all**, exit 0 | **ISSUE 2** — silent drop, should be `-32600`/`id:null` |
| Unknown method `bogus/method` | `-32601` method not found, id echoed, exit 0 | PASS |
| Unknown tool `frobnicate` | `result.isError:true` "unknown tool", exit 0 | PASS |
| `tools/call` missing params / missing name | `isError` "unknown tool ''", exit 0 | PASS (vague msg, no crash) |
| `tools/call` missing/null arguments (recall) | success `[]`, exit 0 | PASS w/ note — see ISSUE 3 |
| Notification (no id), incl. unknown method | no reply, exit 0 | PASS per JSON-RPC |
| Missing `jsonrpc` field (`{"id":1,"method":"ping"}`) | answered `result:{}` | PASS (lenient) |
| `Content-Length: 0` | `-32700` invalid JSON (empty body), exit 0 | PASS |
| `Content-Length: -5` / `abc` / empty | `-32700` "bad Content-Length", exit 0, no hang | PASS |
| `Content-Length: 16777217` (16MiB+1, lying) | `-32700` "exceeds 16777216 byte cap", **no alloc**, exit 0 | PASS |
| `Content-Length: 100` header, no body (half-close) | `-32700` "short read", exit 0, no hang | PASS |
| `Content-Length: 500`, only 10 body bytes (half-close) | `-32700` "short read", exit 0, no hang | PASS |
| Valid framed `Content-Length` ping | `result:{}`, id echoed, exit 0 | PASS |
| Oversize header + pipelined valid msg | error reply, then valid reply; framing resyncs | PASS |
| Two pipelined `Content-Length` msgs / two newline msgs | both answered, exit 0 | PASS |
| Lowercase `content-length:` + body | spurious `-32700` on header line, then body answered as newline msg | PASS w/ note — header match is case-sensitive |
| `remember` missing kind/content/kind=bogus/empty kind/null kind | `isError` unknown-kind or missing-content, exit 0 | PASS |
| `remember` 1MB content | success, id returned, exit 0 | PASS |
| `remember` `importance:NaN` literal | `-32700` (NaN is invalid JSON), exit 0 | PASS |
| `remember` `importance:"high"` (string) | **silently ignored**, stored with default, exit 0 | **ISSUE 3** |
| `recall` `k=-5` / `k="many"` | **silently coerced to default 5**, success, exit 0 | **ISSUE 3** |
| `recall` `k=99999999999` | clamped to 1000, success, exit 0 | PASS |
| `recall` missing/empty `query` | success `[]`, exit 0 (required-field violation not flagged) | **ISSUE 3** |
| `link` missing ids | `isError` "missing 'from'", exit 0 | PASS |
| `link` unknown ids | `isError` "graph link: node not found", exit 0 | PASS |
| `forget`/`purge` missing id | `isError` missing-id, exit 0 | PASS |
| `forget`/`purge` unknown id | success `{"forgotten":false}` / `{"purged":false}`, exit 0 | PASS |
| `related` missing id | `isError` missing-id, exit 0 | PASS |
| `related` unknown id | success `[]` (vs `link` which errors on unknown ids) | **ISSUE 3** (inconsistency) |
| `initialize`, `tools/list`, string/float id echo | correct shapes, id echoed verbatim, exit 0 | PASS |
| `params:[]` (array) on `tools/call` | `isError` "unknown tool ''", exit 0, no panic | PASS |
| Rapid 200× remember+recall pairs, one session | **104/400 replies in 120s, process killed on timeout** — steady progress, 0 `isError`, recall results correct; 10-pair control: 20/20 replies, 0.96 s/op, exit 0 | **ISSUE 1** — embedding-bound (~1 s/op CPU), no deadlock/fd-leak signal |

## Top 3 issues

1. **Bulk throughput, not a hang (worst):** 200 sequential pairs manage ~1 op/s on CPU
   embedding (10-pair control: 0.96 s/op, all correct, 0 errors). The 200-pair run
   reached 104/400 with zero errors before the 120 s harness timeout — progress is
   steady, so this is CPU-inference cost per `remember`/`recall`, not a deadlock or
   fd leak. Still: any rapid/bulk client will time out against this server.
2. **Silent drop of valid non-object JSON:** `42`, `[1,2,3]`, `null`, `"hello"` get
   no reply at all (treated as notifications since they carry no `id`). JSON-RPC
   expects `-32600` Invalid Request with `id:null`. A client sending a batch array
   gets silence instead of an error.
3. **Silent wrong-type/missing-param coercion + one inconsistency:** `recall` with
   missing `query` succeeds with `[]` (schema says required); `k=-5`/`k="many"`
   silently become 5; `importance:"high"` silently ignored; `related` on an unknown
   id succeeds with `[]` while `link` on unknown ids errors. Nothing crashes, but
   typos fail silently instead of loudly.
