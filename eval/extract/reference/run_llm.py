import json, os, resource, statistics as st, sys, time
GGUF, GOLD, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
from llama_cpp import Llama
t0 = time.time()
llm = Llama(model_path=GGUF, n_ctx=2048, n_threads=os.cpu_count(), verbose=False)
load_s = time.time() - t0
rss = lambda: resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024
print(f"load_s={load_s:.2f} rss_mb={rss():.0f}", file=sys.stderr)
SCHEMA = {"type": "object", "properties": {
  "entities": {"type": "array", "items": {"type": "object", "properties": {
      "name": {"type": "string"}, "type": {"type": "string"}}, "required": ["name", "type"]}},
  "relations": {"type": "array", "items": {"type": "object", "properties": {
      "source": {"type": "string"}, "predicate": {"type": "string"}, "target": {"type": "string"},
      "negated": {"type": "boolean"}}, "required": ["source", "predicate", "target", "negated"]}},
  "valid_from": {"type": ["string", "null"]}},
  "required": ["entities", "relations", "valid_from"]}
SYS = ("Extract entities and relations from one memory sentence. Entity names must be exact substrings of the text. "
       "Use predicates like 'lives in', 'works at', 'reports to', 'manages', 'uses', 'owns', 'prefers', 'likes', 'joined', 'moved to'. "
       "Set negated=true when the text says the relation no longer holds or does not hold. "
       "valid_from is an ISO date YYYY-MM-DD when the text states an absolute date, else null. Output JSON only.")
gold = json.load(open(GOLD))["memories"][: int(os.environ.get("LIMIT", "10000"))]
preds, lat = [], []
for i, m in enumerate(gold):
    t = time.time()
    out = llm.create_chat_completion(messages=[{"role": "system", "content": SYS},
                                              {"role": "user", "content": m["content"]}],
                                     response_format={"type": "json_object", "schema": SCHEMA},
                                     temperature=0.0, max_tokens=160)
    lat.append(time.time() - t)
    try:
        p = json.loads(out["choices"][0]["message"]["content"])
    except Exception:
        p = {"entities": [], "relations": [], "valid_from": None}
    preds.append({"entities": p.get("entities", []), "relations": p.get("relations", []),
                  "valid_from": p.get("valid_from")})
    if i % 10 == 9:
        json.dump(preds, open(OUT, "w"), ensure_ascii=False, indent=1)
        print(f"progress {i+1}/{len(gold)} last_s={lat[-1]:.2f}", file=sys.stderr, flush=True)
    if i < 3:
        print(i, m["content"], json.dumps(preds[-1], ensure_ascii=False), file=sys.stderr)
json.dump(preds, open(OUT, "w"), ensure_ascii=False, indent=1)
print(f"n={len(gold)} cold_first_s={lat[0]:.2f} warm_median_s={st.median(lat[1:]):.2f} "
      f"warm_mean_s={st.mean(lat[1:]):.2f} peak_rss_mb={rss():.0f}", file=sys.stderr)
