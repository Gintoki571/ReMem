import json, os, resource, sys, time
os.environ.setdefault("HF_HUB_OFFLINE", "1")
MODEL = "/run/media/bindesh/E/New Folder/rag/gliner2.5-small-v1"
GOLD, OUT = sys.argv[1], sys.argv[2]
ENT = ["person", "place", "organization", "project", "software", "team", "product", "food", "preference"]
REL = ["lives in", "works at", "works for", "reports to", "manages", "uses", "owns", "prefers",
       "likes", "joined", "moved to", "maintains", "teaches at", "reviews"]

def rss_mb():
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024

t0 = time.time()
from gliner2 import AutoExtractor
model = AutoExtractor.from_pretrained(MODEL, map_location="cpu")
load_s = time.time() - t0
print(f"load_s={load_s:.2f} rss_after_load_mb={rss_mb():.0f} class={type(model).__name__}", file=sys.stderr)

gold = json.load(open(GOLD))["memories"]
preds, lat_ent, lat_rel = [], [], []
for i, m in enumerate(gold):
    text = m["content"]
    t = time.time()
    ents_raw = model.extract_entities(text, ENT, include_spans=True)["entities"]
    lat_ent.append(time.time() - t)
    t = time.time()
    rels_raw = model.extract_relations(text, REL, include_spans=True)
    rels_raw = rels_raw.get("relation_extraction", rels_raw)
    lat_rel.append(time.time() - t)
    entities, seen = [], set()
    for label, items in ents_raw.items():
        for it in items:
            if it["text"].lower() not in seen:
                seen.add(it["text"].lower())
                entities.append({"name": it["text"], "type": label})
    relations = []
    for label, items in rels_raw.items():
        for it in items:
            relations.append({"source": it["head"]["text"], "predicate": label.replace("_", " "),
                              "target": it["tail"]["text"], "negated": False})
    preds.append({"entities": entities, "relations": relations})
    if i < 3:
        print(i, text, json.dumps(preds[-1], ensure_ascii=False), file=sys.stderr)

json.dump(preds, open(OUT, "w"), ensure_ascii=False, indent=1)
import statistics as st
cold = lat_ent[0] + lat_rel[0]
warm = [a + b for a, b in zip(lat_ent[1:], lat_rel[1:])]
print(f"n={len(gold)} cold_first_memory_s={cold:.3f} warm_median_s={st.median(warm):.3f} "
      f"warm_mean_s={st.mean(warm):.3f} entity_median_s={st.median(lat_ent):.3f} "
      f"relation_median_s={st.median(lat_rel):.3f} peak_rss_mb={rss_mb():.0f}", file=sys.stderr)
