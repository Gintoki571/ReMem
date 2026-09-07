"""Export cadet-embed-base-v1 (sentence-transformers BERT) to ONNX.

Run: /tmp/embedenv/bin/python scripts/export-onnx.py
"""
import json
import sys
from pathlib import Path

import torch
from safetensors.torch import load_file
from transformers import BertConfig, BertModel

SRC = Path("/home/bindesh/rag/cadet-embed-base-v1")
OUT = Path(__file__).resolve().parent.parent / "models" / "cadet-embed-base-v1"

def main() -> None:
    config = BertConfig.from_pretrained(SRC)
    model = BertModel(config)
    sd = load_file(SRC / "model.safetensors")
    missing, unexpected = model.load_state_dict(sd, strict=False)
    # pooler weights may be absent; mean pooling is applied post-export
    print("missing:", missing)
    print("unexpected:", unexpected)
    model.eval()

    class MeanPool(torch.nn.Module):
        def __init__(self, bert):
            super().__init__()
            self.bert = bert

        def forward(self, input_ids, attention_mask):
            out = self.bert(input_ids=input_ids, attention_mask=attention_mask)
            last = out.last_hidden_state
            mask = attention_mask.unsqueeze(-1).to(last.dtype)
            summed = (last * mask).sum(dim=1)
            counts = mask.sum(dim=1).clamp(min=1e-9)
            mean = summed / counts
            return torch.nn.functional.normalize(mean, p=2, dim=1)

    wrapped = MeanPool(model).eval()
    dummy_ids = torch.ones(1, 8, dtype=torch.long)
    dummy_mask = torch.ones(1, 8, dtype=torch.long)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        wrapped,
        (dummy_ids, dummy_mask),
        str(OUT.with_suffix(".onnx")),
        input_names=["input_ids", "attention_mask"],
        output_names=["embedding_out"],
        dynamic_axes={
            "input_ids": {0: "batch", 1: "seq"},
            "attention_mask": {0: "batch", 1: "seq"},
            "embedding_out": {0: "batch"},
        },
        opset_version=17,
    )
    # copy tokenizer assets next to onnx for transformers.js
    import shutil
    for name in ["tokenizer.json", "tokenizer_config.json", "special_tokens_map.json", "vocab.txt", "config.json"]:
        shutil.copy(SRC / name, OUT.parent / name)
    print("exported to", OUT.with_suffix(".onnx"))

if __name__ == "__main__":
    sys.exit(main())
