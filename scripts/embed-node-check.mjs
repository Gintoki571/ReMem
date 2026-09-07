
// Node-side embedding check for the exported cadet-embed-base-v1 ONNX model.
// Run: node scripts/embed-node-check.mjs  (models/ dir must exist locally, not committed)
import { AutoTokenizer, AutoModel } from "@huggingface/transformers";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const modelsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "models");

const tok = await AutoTokenizer.from_pretrained(modelsDir, { local_files_only: true });
const model = await AutoModel.from_pretrained(modelsDir, {
  dtype: "fp32",
  local_files_only: true,
  file_name: "onnx/model.onnx",
});

async function embed(text) {
  const enc = await tok([text], { return_tensor: true });
  const out = await model({ input_ids: enc.input_ids, attention_mask: enc.attention_mask });
  return Array.from(out.embedding_out.data);
}

function dot(a, b) {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i] * b[i];
  return s;
}

const texts = [
  "my favorite editor is vim",
  "i use vim as my editor",
  "the capital of france is paris",
];
const vecs = [];
for (const t of texts) vecs.push(await embed(t));
console.log("dim:", vecs[0].length);
console.log("cos(vim1,vim2):", dot(vecs[0], vecs[1]).toFixed(3));
console.log("cos(vim1,paris):", dot(vecs[0], vecs[2]).toFixed(3));
if (dot(vecs[0], vecs[1]) < 0.7 || dot(vecs[0], vecs[2]) > 0.7) {
  console.error("FAIL: semantic sanity check failed");
  process.exit(1);
}
const t0 = performance.now();
for (let i = 0; i < 10; i++) await embed("benchmark sentence for latency measurement");
console.log("node ms/text:", ((performance.now() - t0) / 10).toFixed(1));
console.log("OK");
