import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { AutoModel, AutoTokenizer, type PreTrainedModel } from "@huggingface/transformers";

export type EmbedderOptions = {
  modelsDir?: string;
  device?: "webgpu" | "cpu" | "auto";
};

const DEFAULT_MODELS_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "models");

export class Embedder {
  private tokenizer: Awaited<ReturnType<typeof AutoTokenizer.from_pretrained>>;
  private model: PreTrainedModel;
  readonly dims: number;
  readonly device: string;

  private constructor(
    tokenizer: Awaited<ReturnType<typeof AutoTokenizer.from_pretrained>>,
    model: PreTrainedModel,
    dims: number,
    device: string,
  ) {
    this.tokenizer = tokenizer;
    this.model = model;
    this.dims = dims;
    this.device = device;
  }

  static async create(options: EmbedderOptions = {}): Promise<Embedder> {
    const modelsDir = options.modelsDir ?? DEFAULT_MODELS_DIR;
    const tokenizer = await AutoTokenizer.from_pretrained(modelsDir, { local_files_only: true });
    const devices: Array<"webgpu" | "cpu"> =
      options.device === "auto" || options.device === undefined ? ["webgpu", "cpu"] : [options.device];
    let lastError: unknown = null;
    for (const device of devices) {
      try {
        const model = await AutoModel.from_pretrained(modelsDir, {
          dtype: "fp32",
          local_files_only: true,
          model_file_name: "model",
          device,
        });
        const probe = await Embedder.embedOnce(tokenizer, model, "probe");
        return new Embedder(tokenizer, model, probe.length, device);
      } catch (error) {
        lastError = error;
      }
    }
    throw new Error(`no execution provider available for embeddings: ${String(lastError)}`);
  }

  private static async embedOnce(
    tokenizer: Awaited<ReturnType<typeof AutoTokenizer.from_pretrained>>,
    model: PreTrainedModel,
    text: string,
  ): Promise<number[]> {
    const enc = await tokenizer([text], { return_tensor: true, truncation: true, max_length: 512 });
    const out = await model({ input_ids: enc.input_ids, attention_mask: enc.attention_mask });
    return Array.from(out.embedding_out.data);
  }

  async embed(text: string): Promise<number[]> {
    return Embedder.embedOnce(this.tokenizer, this.model, text);
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const results: number[][] = [];
    // ponytail: sequential batch, webgpu adapter handles single-row fast; batch tensor path if throughput matters
    for (const text of texts) {
      results.push(await Embedder.embedOnce(this.tokenizer, this.model, text));
    }
    return results;
  }
}
