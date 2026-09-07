# Models

Local embedding model for ReMem.

## cadet-embed-base-v1

- 768-dim sentence embeddings, BERT-base, mean pooling + L2 normalization (applied inside the exported graph).
- Source: `/home/bindesh/rag/cadet-embed-base-v1` (fine-tuned from intfloat/e5-base-unsupervised).
- Export: `python scripts/export-onnx.py` produces `models/onnx/model.onnx` (+ external data file).
- Not committed (gitignored): `onnx/model.onnx`, `onnx/*.onnx.data`, tokenizer files. Re-export locally or fetch from releases.
- Runtime: `@huggingface/transformers` (transformers.js v4) on Node 22+. Device auto: webgpu then cpu fallback. Measured: ~13 ms/text CPU, ~23 ms/text webgpu (GTX 1660 Ti).
